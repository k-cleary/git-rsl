use git2::{Repository, Remote};

use std::process;

use push_entry::PushEntry;
use rsl::{RSL, HasRSL};
use nonce_bag::{NonceBag, HasNonceBag};
use nonce::{Nonce};
use errors::*;

use utils::git;

pub fn secure_push<'repo>(repo: &Repository, mut remote: &mut Remote, ref_names: &[&str]) -> Result<()> {

    let mut remote_rsl: RSL;
    let mut local_rsl: RSL;
    let mut nonce_bag: NonceBag;
    let mut nonce: Nonce;

    //let mut refs = ref_names.iter().filter_map(|name| &repo.find_reference(name).ok());

    repo.fetch_rsl(&mut remote).chain_err(|| "Problem fetching Remote RSL. Check your connection or your SSH config");

    repo.init_rsl_if_needed(&mut remote).chain_err(|| "Problem initializing RSL");

    // checkout RSL branch
    git::checkout_branch(&repo, "refs/heads/RSL")?;


    'push: loop {

        repo.fetch_rsl(&mut remote).chain_err(|| "Problem fetching Remote RSL. Check your connection or your SSH config");

        let (remote_rsl, local_rsl, nonce_bag, nonce) = match repo.read_rsl() {
            Ok((a,b,c,d)) => (a,b,c,d),
            Err(e) => panic!("Couldn't read RSL: {:?}", e),
        };

        repo.validate_rsl().chain_err(|| "Invalid remote RSL")?;

        // validate that fast forward is possible

        // checkout remote rsl detached
        // make new push entry
        let prev_hash = match remote_rsl.last_push_entry {
            Some(pe) => pe.hash(),
            None => String::from(""),
        };
        //TODO change this to be all ref_names
        let new_push_entry = PushEntry::new(repo, ref_names.first().unwrap(), prev_hash, nonce_bag.clone());
        // TODO commit new pushentry
        repo.commit_push_entry(&new_push_entry).expect("Couldn't commit new push entry");

        // TODO push RSL branch??

        repo.push_rsl(&mut remote)?;

        match git::push(repo, &mut remote, &ref_names) {
            Ok(_) => break 'push,
            Err(e) => {
                println!("Error: unable to push reference(s) {:?} to remote {:?}", &ref_names.clone().join(", "), &remote.name().unwrap());
                println!("  {}", e);
                process::exit(51);
            },
        };
    }
    //TODO localRSL = RemoteRSL (fastforward)
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use utils::test_helper::*;

    #[test]
    fn secure_push() {
        let mut context = setup_fresh();
        {
            let repo = &context.local;
            let mut rem = repo.find_remote("origin").unwrap().to_owned();
            let refs = vec!["master"];
            let res = super::secure_push(&repo, &mut rem, &refs).unwrap();
            assert_eq!(res, ());
        }
        teardown_fresh(context)
    }

    #[test]
    fn secure_push_twice() {
        let mut context = setup_fresh();
        {
            let repo = &context.local;
            let mut rem = repo.find_remote("origin").unwrap().to_owned();
            let refs = &["master"];
            let res = super::secure_push(&repo, &mut rem, refs).unwrap();
            do_work_on_branch(&repo, "master");
            let res2 = super::secure_push(&repo, &mut rem, refs).unwrap();
            assert_eq!(res2, ());
            assert!(false);
        }
        teardown_fresh(context)
    }
}
