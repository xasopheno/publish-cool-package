use std::process::Command;

use anyhow::bail;
use cargo_metadata::Package;

use super::Options;
use crate::utils::will;

pub(in crate::command::release_impl) fn publish_crate(
    publishee: &Package,
    prevent_default_members: bool,
    Options {
        skip_publish,
        dry_run,
        dry_run_cargo_publish,
        allow_dirty,
        no_verify,
        verbose,
        ..
    }: Options,
) -> anyhow::Result<()> {
    if skip_publish {
        return Ok(());
    }
    let max_attempts = 3;
    let uses_cargo_dry_run = dry_run && dry_run_cargo_publish;
    let cargo_must_run = !dry_run || uses_cargo_dry_run;
    for attempt in 1..=max_attempts {
        let mut c = Command::new("cargo");
        c.arg("publish");

        if allow_dirty {
            c.arg("--allow-dirty");
        }
        if no_verify {
            c.arg("--no-verify");
        }
        if uses_cargo_dry_run {
            c.arg("--dry-run");
        }
        c.arg("--manifest-path").arg(&publishee.manifest_path);
        if prevent_default_members {
            c.arg("--package").arg(&publishee.name);
        }
        if verbose {
            log::trace!("{} run {:?}", will(!cargo_must_run), c);
        }
        if !cargo_must_run || c.status()?.success() {
            break;
        } else if attempt == max_attempts || dry_run {
            bail!("Could not successfully execute 'cargo publish'.")
        } else {
            log::warn!(
                "'cargo publish' run {} failed but we retry up to {} times to rule out flakiness",
                attempt,
                max_attempts
            );
        }
    }
    Ok(())
}

pub fn refresh_lock_file() -> anyhow::Result<()> {
    cargo_metadata::MetadataCommand::new().exec()?;
    Ok(())
}
