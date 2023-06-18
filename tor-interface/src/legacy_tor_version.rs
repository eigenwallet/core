// standard
use std::cmp::Ordering;
use std::option::Option;
use std::str::FromStr;
use std::string::ToString;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{}", .0)]
    ParseError(String),
}

// see version-spec.txt
#[derive(Clone)]
pub struct LegacyTorVersion {
    pub major: u32,
    pub minor: u32,
    pub micro: u32,
    pub patch_level: u32,
    pub status_tag: Option<String>,
}

impl LegacyTorVersion {
    fn status_tag_pattern_is_match(status_tag: &str) -> bool {
        if status_tag.is_empty() {
            return false;
        }

        for c in status_tag.chars() {
            if c.is_whitespace() {
                return false;
            }
        }
        true
    }

    fn new(
        major: u32,
        minor: u32,
        micro: u32,
        patch_level: Option<u32>,
        status_tag: Option<&str>,
    ) -> Result<LegacyTorVersion, Error> {
        let status_tag = if let Some(status_tag) = status_tag {
            if Self::status_tag_pattern_is_match(status_tag) {
                Some(status_tag.to_string())
            } else {
                return Err(Error::ParseError(
                    "tor version status tag may not be empty or contain white-space".to_string(),
                ));
            }
        } else {
            None
        };

        Ok(LegacyTorVersion {
            major,
            minor,
            micro,
            patch_level: patch_level.unwrap_or(0u32),
            status_tag,
        })
    }
}

impl FromStr for LegacyTorVersion {
    type Err = Error;

    fn from_str(s: &str) -> Result<LegacyTorVersion, Self::Err> {
        // MAJOR.MINOR.MICRO[.PATCHLEVEL][-STATUS_TAG][ (EXTRA_INFO)]*
        let mut tokens = s.split(' ');
        let (major, minor, micro, patch_level, status_tag) =
            if let Some(version_status_tag) = tokens.next() {
                let mut tokens = version_status_tag.split('-');
                let (major, minor, micro, patch_level) = if let Some(version) = tokens.next() {
                    let mut tokens = version.split('.');
                    let major: u32 = if let Some(major) = tokens.next() {
                        match major.parse() {
                            Ok(major) => major,
                            Err(_) => {
                                return Err(Error::ParseError(format!(
                                    "failed to parse '{}' as MAJOR portion of tor version",
                                    major
                                )))
                            }
                        }
                    } else {
                        return Err(Error::ParseError(
                            "failed to find MAJOR portion of tor version".to_string(),
                        ));
                    };
                    let minor: u32 = if let Some(minor) = tokens.next() {
                        match minor.parse() {
                            Ok(minor) => minor,
                            Err(_) => {
                                return Err(Error::ParseError(format!(
                                    "failed to parse '{}' as MINOR portion of tor version",
                                    minor
                                )))
                            }
                        }
                    } else {
                        return Err(Error::ParseError(
                            "failed to find MINOR portion of tor version".to_string(),
                        ));
                    };
                    let micro: u32 = if let Some(micro) = tokens.next() {
                        match micro.parse() {
                            Ok(micro) => micro,
                            Err(_) => {
                                return Err(Error::ParseError(format!(
                                    "failed to parse '{}' as MICRO portion of tor version",
                                    micro
                                )))
                            }
                        }
                    } else {
                        return Err(Error::ParseError(
                            "failed to find MICRO portion of tor version".to_string(),
                        ));
                    };
                    let patch_level: u32 = if let Some(patch_level) = tokens.next() {
                        match patch_level.parse() {
                            Ok(patch_level) => patch_level,
                            Err(_) => {
                                return Err(Error::ParseError(format!(
                                    "failed to parse '{}' as PATCHLEVEL portion of tor version",
                                    patch_level
                                )))
                            }
                        }
                    } else {
                        0u32
                    };
                    (major, minor, micro, patch_level)
                } else {
                    // if there were '-' the previous next() would have returned the enire string
                    unreachable!();
                };
                let status_tag = tokens.next().map(|status_tag| status_tag.to_string());

                (major, minor, micro, patch_level, status_tag)
            } else {
                // if there were no ' ' character the previou snext() would have returned the enire string
                unreachable!();
            };
        for extra_info in tokens {
            if !extra_info.starts_with('(') || !extra_info.ends_with(')') {
                return Err(Error::ParseError(format!(
                    "failed to parse '{}' as [ (EXTRA_INFO)]",
                    extra_info
                )));
            }
        }
        LegacyTorVersion::new(
            major,
            minor,
            micro,
            Some(patch_level),
            status_tag.as_deref(),
        )
    }
}

impl ToString for LegacyTorVersion {
    fn to_string(&self) -> String {
        match &self.status_tag {
            Some(status_tag) => format!(
                "{}.{}.{}.{}-{}",
                self.major, self.minor, self.micro, self.patch_level, status_tag
            ),
            None => format!(
                "{}.{}.{}.{}",
                self.major, self.minor, self.micro, self.patch_level
            ),
        }
    }
}

impl PartialEq for LegacyTorVersion {
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major
            && self.minor == other.minor
            && self.micro == other.micro
            && self.patch_level == other.patch_level
            && self.status_tag == other.status_tag
    }
}

impl PartialOrd for LegacyTorVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if let Some(order) = self.major.partial_cmp(&other.major) {
            if order != Ordering::Equal {
                return Some(order);
            }
        }

        if let Some(order) = self.minor.partial_cmp(&other.minor) {
            if order != Ordering::Equal {
                return Some(order);
            }
        }

        if let Some(order) = self.micro.partial_cmp(&other.micro) {
            if order != Ordering::Equal {
                return Some(order);
            }
        }

        if let Some(order) = self.patch_level.partial_cmp(&other.patch_level) {
            if order != Ordering::Equal {
                return Some(order);
            }
        }

        // version-spect.txt *does* say that we should compare tags lexicgraphically
        // if all of the version numbers are the same when comparing, but we are
        // going to diverge here and say we can only compare tags for equality.
        //
        // In practice we will be comparing tor daemon tags against tagless (stable)
        // versions so this shouldn't be an issue

        if self.status_tag == other.status_tag {
            return Some(Ordering::Equal);
        }

        None
    }
}

#[test]
fn test_version() -> anyhow::Result<()> {
    assert!(LegacyTorVersion::from_str("1.2.3")? == LegacyTorVersion::new(1, 2, 3, None, None)?);
    assert!(
        LegacyTorVersion::from_str("1.2.3.4")? == LegacyTorVersion::new(1, 2, 3, Some(4), None)?
    );
    assert!(
        LegacyTorVersion::from_str("1.2.3-test")?
            == LegacyTorVersion::new(1, 2, 3, None, Some("test"))?
    );
    assert!(
        LegacyTorVersion::from_str("1.2.3.4-test")?
            == LegacyTorVersion::new(1, 2, 3, Some(4), Some("test"))?
    );
    assert!(
        LegacyTorVersion::from_str("1.2.3 (extra_info)")?
            == LegacyTorVersion::new(1, 2, 3, None, None)?
    );
    assert!(
        LegacyTorVersion::from_str("1.2.3.4 (extra_info)")?
            == LegacyTorVersion::new(1, 2, 3, Some(4), None)?
    );
    assert!(
        LegacyTorVersion::from_str("1.2.3.4-tag (extra_info)")?
            == LegacyTorVersion::new(1, 2, 3, Some(4), Some("tag"))?
    );

    assert!(
        LegacyTorVersion::from_str("1.2.3.4-tag (extra_info) (extra_info)")?
            == LegacyTorVersion::new(1, 2, 3, Some(4), Some("tag"))?
    );

    assert!(LegacyTorVersion::new(1, 2, 3, Some(4), Some("spaced tag")).is_err());
    assert!(LegacyTorVersion::new(1, 2, 3, Some(4), Some("" /* empty tag */)).is_err());
    assert!(LegacyTorVersion::from_str("").is_err());
    assert!(LegacyTorVersion::from_str("1.2").is_err());
    assert!(LegacyTorVersion::from_str("1.2-foo").is_err());
    assert!(LegacyTorVersion::from_str("1.2.3.4-foo bar").is_err());
    assert!(LegacyTorVersion::from_str("1.2.3.4-foo bar (extra_info)").is_err());
    assert!(LegacyTorVersion::from_str("1.2.3.4-foo (extra_info) badtext").is_err());
    assert!(
        LegacyTorVersion::new(0, 0, 0, Some(0), None)?
            < LegacyTorVersion::new(1, 0, 0, Some(0), None)?
    );
    assert!(
        LegacyTorVersion::new(0, 0, 0, Some(0), None)?
            < LegacyTorVersion::new(0, 1, 0, Some(0), None)?
    );
    assert!(
        LegacyTorVersion::new(0, 0, 0, Some(0), None)?
            < LegacyTorVersion::new(0, 0, 1, Some(0), None)?
    );

    // ensure status tags make comparison between equal versions (apart from
    // tags) unknowable
    let zero_version = LegacyTorVersion::new(0, 0, 0, Some(0), None)?;
    let zero_version_tag = LegacyTorVersion::new(0, 0, 0, Some(0), Some("tag"))?;

    assert!(!(zero_version < zero_version_tag));
    assert!(!(zero_version <= zero_version_tag));
    assert!(!(zero_version > zero_version_tag));
    assert!(!(zero_version >= zero_version_tag));

    Ok(())
}
