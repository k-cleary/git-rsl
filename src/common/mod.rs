

extern crate crypto;
extern crate rand;

use std::{env, process};
use std::vec::Vec;
use std::collections::HashSet;
use std::iter::FromIterator;


use git2;
use git2::{Error, FetchOptions, PushOptions, Oid, Reference, Branch, Commit, RemoteCallbacks, Remote, Repository, Revwalk, DiffOptions, RepositoryState};
use git2::build::CheckoutBuilder;
use git2::BranchType;

use git2::StashApplyOptions;
use git2::STASH_INCLUDE_UNTRACKED;


pub mod push_entry;
pub mod nonce;
pub mod nonce_bag;
pub mod rsl;

pub use self::push_entry::PushEntry;
pub use self::nonce::{Nonce, HasNonce};
pub use self::nonce_bag::{NonceBag, HasNonceBag};
pub use self::rsl::{RSL, HasRSL};

pub mod errors {
    error_chain!{
        foreign_links {
            Git(::git2::Error);
            Serde(::serde_json::Error);
            IO(::std::io::Error);
        }
    }
}

use self::errors::*;




pub fn fetch(repo: &Repository, remote: &mut Remote, ref_names: &[&str], _reflog_msg: Option<&str>) -> Result<()> {
    let cfg = repo.config().unwrap();
    let remote_copy = remote.clone();
    let url = remote_copy.url().unwrap();

    with_authentication(url, &cfg, |f| {

        let mut cb = RemoteCallbacks::new();
        cb.credentials(f);
        let mut opts = FetchOptions::new();
        opts.remote_callbacks(cb);

        let reflog_msg = "Retrieve RSL branch from remote";

        remote.fetch(&ref_names, Some(&mut opts), Some(&reflog_msg)).chain_err(|| "could not fetch ref")
    })
}

pub fn push(repo: &Repository, remote: &mut Remote, ref_names: &[&str]) -> Result<()> {
    let cfg = repo.config().unwrap();
    let remote_copy = remote.clone();
    let url = remote_copy.url().unwrap();

    with_authentication(url, &cfg, |f| {
        let mut cb = RemoteCallbacks::new();
        cb.credentials(|a,b,c| f(a,b,c));
        let mut opts = PushOptions::new();
        opts.remote_callbacks(cb);

        let mut refs: Vec<String> = ref_names
            .to_vec()
            .iter()
            .map(|name: &&str| format!("refs/heads/{}:refs/heads/{}", name.to_string(), name.to_string()))
            .collect();

        let mut refs_ref: Vec<&str> = vec![];
        for name in &refs {
            refs_ref.push(&name)
        }

        remote.push(&refs_ref, Some(&mut opts))?;
        Ok(())
    })
}

// pub fn retrieve_rsl_and_nonce_bag_from_remote_repo<'repo>(repo: &'repo Repository, mut remote: &mut Remote) -> Option<(Reference<'repo>, NonceBag)> {
//
//     fetch(repo, remote, &[RSL_BRANCH], Some(REFLOG_MSG));
//     let remote_rsl = match repo.find_branch(RSL_BRANCH, BranchType::Remote) {
//             Err(e) => return None,
//             Ok(rsl) => (rsl.into_reference())
//         };
//
//     let nonce_bag = match repo.read_nonce_bag(&remote_rsl) {
//         Ok(n) => n,
//         Err(_) => process::exit(10),
//     };
//
//     let repo_nonce = match repo.read_nonce() {
//         Ok(nonce) => nonce,
//         Err(e) => {
//             println!("Error: Couldn't read nonce: {:?}", e);
//             return false;
//         },
//     };
//     Some((remote_rsl, local_rsl, nonce_bag, repo_nonce))
// }


pub fn all_push_entries_in_fetch_head(repo: &Repository, ref_names: &Vec<&str>) -> bool {

    let mut latest_push_entries: &Vec<git2::Oid> = &ref_names.clone().into_iter().filter_map(|ref_name| {
        match last_push_entry_for(repo, ref_name) {
            Some(pe) => Some(pe.head),
            None => None,
        }
    }).collect();
    let mut fetch_heads : &Vec<git2::Oid> = &ref_names.clone().into_iter().filter_map(|ref_name| {
        match repo.find_branch(ref_name, BranchType::Remote) {
            Ok(branch) => branch.get().target(),
            Err(_) => None
        }
    }).collect();
    let h1: HashSet<&git2::Oid> = HashSet::from_iter(latest_push_entries);
    let h2: HashSet<&git2::Oid> = HashSet::from_iter(fetch_heads);

    h2.is_subset(&h1)
}

pub fn validate_rsl(repo: &Repository, remote_rsl: &RSL, local_rsl: &RSL, nonce_bag: &NonceBag, repo_nonce: &Nonce) -> Result<()> {

    // Ensure remote RSL head is a descendant of local RSL head.
    let descendant = repo
        .graph_descendant_of(remote_rsl.head, local_rsl.head)
        .unwrap_or(false);
    let same = (remote_rsl.head == local_rsl.head);
    if !descendant && !same {
        bail!("RSL invalid: No path to get from Local RSL to Remote RSL");
    }

    // Walk through the commits from local RSL head, which we know is valid,
    // validating each additional pushentry since that point one by one.
    let last_hash = match local_rsl.last_push_entry {
        Some(ref push_entry) => Some(push_entry.hash()),
        None => None, // the first push entry will have None as last_push_entry
    };
    let mut revwalk: Revwalk = repo.revwalk().unwrap();
    revwalk.push(remote_rsl.head);
    revwalk.set_sorting(git2::SORT_REVERSE);
    revwalk.hide(local_rsl.head);

    let remaining = revwalk.map(|oid| oid.unwrap());

    let result = remaining.fold(last_hash, |prev_hash, oid| {
        match PushEntry::from_oid(&repo, &oid) {
            Some(current_push_entry) => {
                let current_prev_hash = current_push_entry.prev_hash();

                // if current prev_hash == local_rsl.head (that is, we have arrived at the first push entry after the last recorded one), then check if repo_nonce in PushEntry::from_oid(oid.parent_commit) or noncebag contains repo_nonce; return false if neither holds
                //if current_prev_hash == last_local_push_entry.hash() {

                    // validate nonce bag (lines 1-2):
                    // TODO does this take care of when there haven't been any new entries or only one new entry?
                    //if !nonce_bag.bag.contains(&repo_nonce) && !current_push_entry.nonce_bag.bag.contains(&repo_nonce) { // repo nonce not in remote nonce bag && repo_nonce not in remote_rsl.push_after(local_rsl){
                    //    None;
                    //}
                //}
                let current_hash = current_push_entry.hash();
                if prev_hash == Some(current_prev_hash) {
                    Some(current_hash)
                } else {
                    None
                }
            },
            None => prev_hash, // this was not a pushentry. continue with previous entry in hand
        }

    });

    if result != None { bail!("invalid RSL entry"); }


    verify_signature(remote_rsl.head).chain_err(|| "GPG signature of remote RSL head invalid")

}

fn verify_signature(_oid: Oid) -> Result<()> {
    return Ok(())
}

fn for_each_commit_from<F>(repo: &Repository, local: Oid, remote: Oid, f: F)
    where F: Fn(Oid) -> ()
{
    let mut revwalk: Revwalk = repo.revwalk().unwrap();
    revwalk.push(remote);
    revwalk.set_sorting(git2::SORT_REVERSE);
    revwalk.hide(local);
    let remaining = revwalk.map(|oid| oid.unwrap());

    for oid in remaining {
        f(oid)
    }
}

fn find_first_commit(repo: &Repository) -> Result<Commit> {
    let mut revwalk: Revwalk = repo.revwalk().expect("Failed to make revwalk");
    revwalk.push_head();
    let result = revwalk
        .last()  // option<result<oid, err>> => result<oid, err>
        .ok_or("revwalk empty?")? // result<oid, err>
        .chain_err(|| "revwalk gave error")?;
    let commit = repo.find_commit(result).chain_err(|| "first commit not in repo");
    commit
}

// pub fn local_rsl_from_repo(repo: &Repository) -> Option<Reference> {
//     match repo.find_reference(RSL_BRANCH) {
//         Ok(r) => Some(r),
//         Err(_) => None,
//     }
// }




pub fn last_push_entry_for(repo: &Repository, reference: &str) -> Option<PushEntry> {
    //TODO Actually walk the commits and look for the most recent for the branch we're interested
    //in

    // this is where it might come in yuseful to keep track of the last push entry for a branch...
    // for each ref, try to parse into a pushentry
    /// if you can, check if that pushentry is for the branch
    // if it is , return that pushentry. otherwise keep going
    // if you get to then end of the walk, return false
    Some(PushEntry::new(repo, reference, String::from(""), NonceBag::new()))
}

//TODO implement
pub fn reset_local_rsl_to_remote_rsl(_repo: &Repository) {
}



fn with_authentication<T, F>(url: &str, cfg: &git2::Config, mut f: F)
                             -> Result<T>
    where F: FnMut(&mut git2::Credentials) -> Result<T>
{
    let mut cred_helper = git2::CredentialHelper::new(url);
    cred_helper.config(cfg);

    let mut ssh_username_requested = false;
    let mut cred_helper_bad = None;
    let mut ssh_agent_attempts = Vec::new();
    let mut any_attempts = false;
    let mut tried_sshkey = false;

    let mut res = f(&mut |url, username, allowed| {
        any_attempts = true;
        // libgit2's "USERNAME" authentication actually means that it's just
        // asking us for a username to keep going. This is currently only really
        // used for SSH authentication and isn't really an authentication type.
        // The logic currently looks like:
        //
        //      let user = ...;
        //      if (user.is_null())
        //          user = callback(USERNAME, null, ...);
        //
        //      callback(SSH_KEY, user, ...)
        //
        // So if we're being called here then we know that (a) we're using ssh
        // authentication and (b) no username was specified in the URL that
        // we're trying to clone. We need to guess an appropriate username here,
        // but that may involve a few attempts. Unfortunately we can't switch
        // usernames during one authentication session with libgit2, so to
        // handle this we bail out of this authentication session after setting
        // the flag `ssh_username_requested`, and then we handle this below.
        if allowed.contains(git2::USERNAME) {
            debug_assert!(username.is_none());
            ssh_username_requested = true;
            bail!(git2::Error::from_str("gonna try usernames later"))
        }

        // An "SSH_KEY" authentication indicates that we need some sort of SSH
        // authentication. This can currently either come from the ssh-agent
        // process or from a raw in-memory SSH key. Cargo only supports using
        // ssh-agent currently.
        //
        // If we get called with this then the only way that should be possible
        // is if a username is specified in the URL itself (e.g. `username` is
        // Some), hence the unwrap() here. We try custom usernames down below.
        if allowed.contains(git2::SSH_KEY) && !tried_sshkey {
            // If ssh-agent authentication fails, libgit2 will keep
            // calling this callback asking for other authentication
            // methods to try. Make sure we only try ssh-agent once,
            // to avoid looping forever.
            tried_sshkey = true;
            let username = username.unwrap();
            debug_assert!(!ssh_username_requested);
            ssh_agent_attempts.push(username.to_string());
            return git2::Cred::ssh_key_from_agent(username)
        }

        // Sometimes libgit2 will ask for a username/password in plaintext. This
        // is where Cargo would have an interactive prompt if we supported it,
        // but we currently don't! Right now the only way we support fetching a
        // plaintext password is through the `credential.helper` support, so
        // fetch that here.
        if allowed.contains(git2::USER_PASS_PLAINTEXT) {
            let r = git2::Cred::credential_helper(cfg, url, username);
            cred_helper_bad = Some(r.is_err());
            return r
        }

        // I'm... not sure what the DEFAULT kind of authentication is, but seems
        // easy to support?
        if allowed.contains(git2::DEFAULT) {
            return git2::Cred::default()
        }

        // Whelp, we tried our best
        bail!(git2::Error::from_str("no authentication available"))
    });


    // Ok, so if it looks like we're going to be doing ssh authentication, we
    // want to try a few different usernames as one wasn't specified in the URL
    // for us to use. In order, we'll try:
    //
    // * A credential helper's username for this URL, if available.
    // * This account's username.
    // * "git"
    //
    // We have to restart the authentication session each time (due to
    // constraints in libssh2 I guess? maybe this is inherent to ssh?), so we
    // call our callback, `f`, in a loop here.
    if ssh_username_requested {
        debug_assert!(res.is_err());
        let mut attempts = Vec::new();
        attempts.push("git".to_string());
        if let Ok(s) = ::std::env::var("USER").or_else(|_| ::std::env::var("USERNAME")) {
            attempts.push(s);
        }
        if let Some(ref s) = cred_helper.username {
            attempts.push(s.clone());
        }

        while let Some(s) = attempts.pop() {
            // We should get `USERNAME` first, where we just return our attempt,
            // and then after that we should get `SSH_KEY`. If the first attempt
            // fails we'll get called again, but we don't have another option so
            // we bail out.
            let mut attempts = 0;
            res = f(&mut |_url, username, allowed| {
                if allowed.contains(git2::USERNAME) {
                    println!("username: {}", &s);

                    return git2::Cred::username(&s);
                }
                if allowed.contains(git2::SSH_KEY) {
                    debug_assert_eq!(Some(&s[..]), username);
                    attempts += 1;
                    if attempts == 1 {
                        ssh_agent_attempts.push(s.to_string());
                        return git2::Cred::ssh_key_from_agent(&s)
                    }
                }
                bail!(git2::Error::from_str("no authentication available"));
            });


            // If we made two attempts then that means:
            //
            // 1. A username was requested, we returned `s`.
            // 2. An ssh key was requested, we returned to look up `s` in the
            //    ssh agent.
            // 3. For whatever reason that lookup failed, so we were asked again
            //    for another mode of authentication.
            //
            // Essentially, if `attempts == 2` then in theory the only error was
            // that this username failed to authenticate (e.g. no other network
            // errors happened). Otherwise something else is funny so we bail
            // out.
            if attempts != 2 {
                break
            }
        }
    }

    if res.is_ok() || !any_attempts {
        return res.map_err(From::from)
    }

    // In the case of an authentication failure (where we tried something) then
    // we try to give a more helpful error message about precisely what we
    // tried.
    res
}


#[cfg(test)]
mod tests {
    use super::*;
    use utils::test_helper::*;



}
