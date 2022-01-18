//! Build time version generator based on the state of gitflow
//! Scans the current commit, tags, and branch names to determine the version in a particular git
//! repository. 
//!
//! This project supports a subset of semantic versioning to describe branches. It supports four
//! kinds of versions:
//! 1. Production: Indicates a production version with a major, minor, and patch version
//! 2. Alpha: Indicates a near-production ready version which is in testing.
//!    Contains a major, minor, patch, and release candidate version
//! 3. Development: Indicates a build on a develop branch
//! 4. Local: A local build on a feature branch
//!
//! These versions can each be determined by the state of gitflow. Initially, a developer will
//! checkout a feature branch, leading to the version always being `Local`. A feature branch is
//! defined as a branch that doesn't fall under any of the following categories. 
//!
//! Once the feature is complete, the developer will merge their branch into develop. When this
//! product is build, this will constitute a `Development` version.
//!
//! Later a release branch will be checked out based off a commit on develop branch.
//! The name of this branch must be in the format `vX.Y.Z`, where X, Y, and Z can any number of
//! base 10 digits. This version will be the base semver version for all future commits on this
//! branch. The first commit will be release candidate 1 leading to a full version of `vX.Y.Z-rc.1`.
//! This assumes that each commit that is released is tagged with its corresponding version so that
//! when future commits are pushed, the release candidate number is incremented from the last known
//! tag on the version branch. 
//!
//! Finally once sufficient testing has occurred, a commit on the version branch will be merged
//! onto master/main to do a full release. This version will be based on the parent commits'
//! version branch name. 
//!
//! TODO: Hotfix branch and how that works

use git2::{Branch, Commit};
use serde::{Deserialize, Serialize};
use std::{fmt::Display, path::Path};

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Copy, Clone, Hash)]
pub struct SemverBase {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Copy, Clone, Hash)]
pub struct SemverRC {
    pub base: SemverBase,
    pub rc: u8,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Copy, Clone, Hash)]
pub enum VersionInfo {
    /// Production release (master branch)
    Production(SemverBase),

    /// Alpha release (vX.Y.Z branch)
    /// Produced by vX.Y.Z-rc.W versions
    Alpha(SemverRC),

    /// Development release (develop branch)
    Development,

    /// Build for local testing, feature branch
    Local,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone, Hash)]
pub struct GitflowInfo {
    pub branch_name: String,
    pub version: VersionInfo,
    pub commit_hash: String,
    pub build_number: u64,
}

impl Display for SemverBase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Display for SemverRC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "v{}.{}.{}-rc.{}",
            self.base.major, self.base.minor, self.base.patch, self.rc
        )
    }
}

impl Display for VersionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            VersionInfo::Production(semver) => write!(f, "Prod: {}", semver),
            VersionInfo::Alpha(semver) => write!(f, "Alpha: {}", semver),
            VersionInfo::Development => write!(f, "Development"),
            VersionInfo::Local => write!(f, "Local"),
        }
    }
}

impl VersionInfo {
    pub fn get_semver(&self) -> Option<String> {
        match &self {
            VersionInfo::Production(semver) => Some(format!("{}", semver)),
            VersionInfo::Alpha(semver) => Some(format!("{}", semver)),
            VersionInfo::Development => None,
            VersionInfo::Local => None,
        }
    }

    pub fn is_production(&self) -> bool {
        matches!(self, &VersionInfo::Production(_))
    }

    pub fn is_alpha(&self) -> bool {
        matches!(self, &VersionInfo::Alpha(_))
    }
}

fn parse_semver(semver: &str) -> Result<VersionInfo, Box<dyn std::error::Error>> {
    if !semver.starts_with('v') {
        return Err("Semver must start with a v".into());
    }
    let semver = &semver[1..];
    let version = semver::Version::parse(semver)?;
    if !version.build.is_empty() {
        return Err("Semver must not contain a build identifier".into());
    }

    let pre = version.pre.as_str();
    let base = SemverBase {
        major: version.major.try_into()?,
        minor: version.minor.try_into()?,
        patch: version.patch.try_into()?,
    };
    if pre.is_empty() {
        Ok(VersionInfo::Production(base))
    } else {
        let mut parts = pre.split('.');
        let first = parts.next().unwrap();
        let second = parts.next().ok_or("Expected rc.W at end of version")?;

        if first == "rc" {
            let rc: u8 = second.parse()?;
            Ok(VersionInfo::Alpha(SemverRC { base, rc }))
        } else {
            Err(format!("Unsupported prerelease: {first}").into())
        }
    }
}

pub fn get_info_from_path(path: &Path) -> Result<GitflowInfo, Box<dyn std::error::Error>> {
    let repo = git2::Repository::open(&path)?;
    let head = repo.head()?;
    let head_commit = head.peel_to_commit()?;
    let commit_hash = hex::encode(head_commit.id().as_bytes());

    let branches: Vec<Branch> = repo
        .branches(None)?
        .filter_map(|branch| {
            branch
                .map(|(branch, _kind)| if branch.is_head() { Some(branch) } else { None })
                .ok()
                .flatten()
        })
        .collect();

    if branches.len() > 1 {
        return Err("Commit on too many branches!".into());
    }
    if branches.is_empty() {
        return Err("Commit {commit_hash} on no branch".into());
    }
    let branch = branches.into_iter().next().unwrap();
    let branch_name = branch.name()?.unwrap();

    // Count the number of parent commits
    let build_number: usize = count_parents(&head_commit);
    fn count_parents(commit: &Commit) -> usize {
        let mut count = 0;
        for commit in commit.parents() {
            count += count_parents(&commit);
        }
        count
    }

    let output = "";
    Ok(GitflowInfo {
        branch_name: branch_name.to_owned(),
        version: parse_semver(output)
            .unwrap_or_else(|_| panic!("failed to parse semver: {}", output)),
        commit_hash,
        build_number: build_number as u64,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_release_1() {
        assert_eq!(
            parse_semver("v0.1.0").unwrap(),
            VersionInfo::Production(SemverBase {
                major: 0,
                minor: 1,
                patch: 0,
            })
        );
    }

    #[test]
    fn parse_release_2() {
        assert_eq!(
            parse_semver("v3.2.1").unwrap(),
            VersionInfo::Production(SemverBase {
                major: 3,
                minor: 2,
                patch: 1,
            })
        );
    }

    #[test]
    fn parse_alpha_1() {
        assert_eq!(
            parse_semver("v1.2.3-rc.9").unwrap(),
            VersionInfo::Alpha(SemverRC {
                base: SemverBase {
                    major: 1,
                    minor: 2,
                    patch: 3,
                },
                rc: 9,
            })
        );
    }

    #[test]
    fn parse_alpha_2() {
        assert_eq!(
            parse_semver("v1.13.4-rc.1").unwrap(),
            VersionInfo::Alpha(SemverRC {
                base: SemverBase {
                    major: 1,
                    minor: 13,
                    patch: 4,
                },
                rc: 1,
            })
        );
    }

    #[test]
    fn parse_bad_1() {
        assert!(parse_semver("O1").is_err());
    }

    #[test]
    fn parse_bad_2() {
        assert!(parse_semver("").is_err());
    }

    #[test]
    fn parse_bad_3() {
        assert!(parse_semver("A").is_err());
    }

    #[test]
    fn parse_bad_4() {
        assert!(parse_semver("1.1").is_err());
    }
}
