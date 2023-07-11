use std::path::PathBuf;

use clap::Parser;
use itertools::Itertools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Command::parse().run().await
}

#[derive(Parser)]
struct Command {
    #[clap(short = 'f', default_value = "spin.toml")]
    from: PathBuf,
}

impl Command {
    async fn run(&self) -> anyhow::Result<()> {
        let manifest_path = spin_common::paths::resolve_manifest_file_path(&self.from)?;
        let manifest = spin_loader::local::raw_manifest_from_file(&manifest_path).await?.into_v1();
        let components = manifest.components;
        let interestings = components.into_iter().filter_map(Interesting::maybe_from).collect_vec();

        let available_upgrade_futs = interestings.iter().map(|i| i.check_upgrade());
        let au2 = futures::future::join_all(available_upgrade_futs).await;
        let au3 = au2.into_iter().filter_map(|u| u).collect_vec();

        if au3.is_empty() {
            eprintln!("No upgrades found");
        } else {
            for au in au3 {
                println!("{}: uses {}/{}: current: {}; available: {}", au.component_id, au.repo_owner, au.repo_name, au.current, au.latest);
            }
        }

        Ok(())
    }
}

struct Interesting {
    component_id: String,
    source_url: String,
}

impl Interesting {
    fn maybe_from<T>(source: spin_loader::local::config::RawComponentManifestImpl<T>) -> Option<Self> {
        match source.source {
            spin_loader::local::config::RawModuleSource::Url(u) => Some(Self { component_id: source.id, source_url: u.url }),
            _ => None,
        }
    }

    async fn check_upgrade(&self) -> Option<AvailableUpgrade> {
        let Some(gh_release) = gh_release(&self.source_url) else {
            return None;
        };

        let gh = octocrab::instance();
        let repo = gh.repos(&gh_release.repo_owner, &gh_release.repo_name);
        let releases = repo.releases();
        let Ok(release) = releases.get_latest().await else {
            eprintln!("Couldn't get latest release for {}/{}", gh_release.repo_owner, gh_release.repo_name);
            return None;
        };

        if release.tag_name == gh_release.version {
            return None;
        }

        return Some(AvailableUpgrade::new(self.component_id.clone(), gh_release, release.tag_name))
    }
}

fn gh_release(url: &str) -> Option<GitHubRelease> {
    url::Url::parse(url).ok().and_then(|url| {
        if url.host_str() != Some("github.com") {
            return None;
        }
        let Some(segments) = url.path_segments() else {
            return None;
        };
        let segments = segments.collect_vec();
        let mut repo = segments.iter().take_while(|seg| seg != &&"releases").map(|s| s.to_string());
        let Some(repo_owner) = repo.next() else {
            return None;
        };
        let Some(repo_name) = repo.next() else {
            return None;
        };
        let Some(version) = segments.iter().skip_while(|seg| seg != &&"download").nth(1) else {
            return None;
        };

        Some(GitHubRelease { repo_owner, repo_name, version: version.to_string() })
    })
}

struct GitHubRelease {
    repo_owner: String,
    repo_name: String,
    version: String,
}

struct AvailableUpgrade {
    component_id: String,
    repo_owner: String,
    repo_name: String,
    current: String,
    latest: String,
}

impl AvailableUpgrade {
    fn new(component_id: String, from: GitHubRelease, latest: String) -> Self {
        Self {
            component_id,
            repo_owner: from.repo_owner,
            repo_name: from.repo_name,
            current: from.version,
            latest,
        }
    }
}
