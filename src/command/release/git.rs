use std::process::Command;

use anyhow::bail;
use git_repository::Id;

use crate::utils::will;

pub(in crate::command::release_impl) fn commit_changes(
    message: impl AsRef<str>,
    dry_run: bool,
    empty_commit_possible: bool,
    ctx: &crate::Context,
) -> anyhow::Result<Option<Id<'_>>> {
    // TODO: replace with gitoxide one day
    let mut cmd = Command::new("git");
    cmd.arg("commit").arg("-am").arg(message.as_ref());
    if empty_commit_possible {
        cmd.arg("--allow-empty");
    }
    log::trace!("{} run {:?}", will(dry_run), cmd);
    if dry_run {
        return Ok(None);
    }

    if !cmd.status()?.success() {
        bail!("Failed to commit changed manifests");
    }
    Ok(Some(
        ctx.repo.find_reference("HEAD")?.peel_to_id_in_place()?,
    ))
}
