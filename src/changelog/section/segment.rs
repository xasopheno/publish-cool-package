use std::collections::BTreeMap;

use bitflags::bitflags;

pub mod details {
    use std::fmt;

    use git_repository as git;

    #[derive(PartialEq, Eq, Ord, PartialOrd, Debug, Clone)]
    pub enum Category {
        Issue(String),
        Uncategorized,
    }

    impl fmt::Display for Category {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Category::Uncategorized => f.write_str("Uncategorized"),
                Category::Issue(issue) => write!(f, "#{}", issue),
            }
        }
    }

    #[derive(PartialEq, Eq, Debug, Clone)]
    pub struct Message {
        pub title: String,
        pub id: git::ObjectId,
    }

    impl From<&crate::commit::history::Item> for Message {
        fn from(v: &crate::commit::history::Item) -> Self {
            Message {
                title: v.message.title.to_owned(),
                id: v.id,
            }
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Details {
    pub commits_by_category: BTreeMap<details::Category, Vec<details::Message>>,
}

impl Details {
    pub const TITLE: &'static str = "Commit Details";
    pub const HTML_PREFIX: &'static str = "<details><summary>view details</summary>";
    pub const HTML_PREFIX_END: &'static str = "</details>";
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct CommitStatistics {
    /// Amount of commits that contributed to the release
    pub count: usize,
    /// The time span from first to last commit, if there is more than one.
    pub duration: Option<time::Duration>,
    /// The issue numbers that were referenced in commit messages
    pub unique_issues: Vec<details::Category>,
    /// The duration from the release before this one, if this isn't the first release.
    pub time_passed_since_last_release: Option<time::Duration>,
}

impl CommitStatistics {
    pub const TITLE: &'static str = "Commit Statistics";
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct ThanksClippy {
    pub count: usize,
}

impl ThanksClippy {
    pub const TITLE: &'static str = "Thanks Clippy";
}

bitflags! {
    pub struct Selection: u8 {
        const CLIPPY = 1<<0;
        const COMMIT_DETAILS = 1<<1;
        const COMMIT_STATISTICS = 1<<2;
    }
}
