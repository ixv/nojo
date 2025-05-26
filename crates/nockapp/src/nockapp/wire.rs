use nockvm::noun::{Noun, NounAllocator, D, T};

use crate::utils::make_tas;

/// Standardized wire format for kernel interaction.
pub trait Wire: Sized {
    /// Protocol version
    const VERSION: u64;

    /// Driver/Module identifier
    const SOURCE: &'static str;

    /// Get wire for this driver. Specific implementations can add more tags to the wire as they wish but the default is just [`Self::SOURCE`] and [`Self::VERSION`].
    fn to_wire(&self) -> WireRepr {
        WireRepr::no_tags(Self::SOURCE, Self::VERSION)
    }
}

/// Converts a wire to a Noun by allocating it on the kernel's stack.
pub(crate) fn wire_to_noun<A: NounAllocator>(stack: &mut A, wire: &WireRepr) -> Noun {
    let source_atom = make_tas(stack, wire.source);
    let version_atom: Noun = D(wire.version);
    if wire.tags.is_empty() {
        T(stack, &[source_atom.as_noun(), version_atom, D(0)])
    } else {
        let mut wire_noun = Vec::with_capacity(wire.tags.len() + 3);
        wire_noun.push(make_tas(stack, wire.source).as_noun());
        wire_noun.push(D(wire.version));
        for tag in &wire.tags {
            wire_noun.push(tag.as_noun(stack));
        }
        wire_noun.push(D(0));

        T(stack, &wire_noun)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireTag {
    Direct(u64),
    String(String),
}

impl WireTag {
    pub fn as_noun<A: NounAllocator>(&self, stack: &mut A) -> Noun {
        match self {
            WireTag::Direct(d) => D(*d),
            WireTag::String(s) => make_tas(stack, s).as_noun(),
        }
    }
}

impl std::fmt::Display for WireTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireTag::Direct(d) => write!(f, "{}", d),
            WireTag::String(s) => write!(f, "{}", s),
        }
    }
}

impl From<u8> for WireTag {
    fn from(d: u8) -> Self {
        WireTag::Direct(d as u64)
    }
}
impl From<String> for WireTag {
    fn from(s: String) -> Self {
        WireTag::String(s)
    }
}

impl From<u64> for WireTag {
    fn from(d: u64) -> Self {
        WireTag::Direct(d)
    }
}

impl From<&u64> for WireTag {
    fn from(d: &u64) -> Self {
        WireTag::Direct(*d)
    }
}

impl From<&str> for WireTag {
    fn from(s: &str) -> Self {
        WireTag::String(s.to_string())
    }
}

/// WireRepr is intended to make the default scenario (no custom tags beyond source and version) not allocate on the heap up-front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireRepr {
    pub source: &'static str,
    pub version: u64,
    pub tags: Vec<WireTag>,
}

impl WireRepr {
    pub fn new(source: &'static str, version: u64, tags: Vec<WireTag>) -> Self {
        WireRepr {
            source,
            version,
            tags,
        }
    }
    pub fn no_tags(source: &'static str, version: u64) -> Self {
        WireRepr {
            source,
            version,
            tags: Vec::new(),
        }
    }
    pub fn tags_as_csv(&self) -> String {
        let mut tags = Vec::with_capacity(self.tags.len() + 2);
        tags.push(self.source.to_string());
        tags.push(self.version.to_string());
        for tag in &self.tags {
            match tag {
                WireTag::Direct(d) => tags.push(d.to_string()),
                WireTag::String(s) => tags.push(s.clone()),
            }
        }
        tags.join(",")
    }
}

/// System wire to use when no other wire is specified
pub struct SystemWire;

impl Wire for SystemWire {
    const VERSION: u64 = 1;
    const SOURCE: &'static str = "sys";
}

#[cfg(test)]
mod test {
    use nockvm_macros::tas;
    use tracing::debug;

    use crate::noun::slab::NounSlab;

    use super::*;

    enum NpcWire {
        Poke(u64),
        Pack(u64),
        Nack(u64),
        Bind(u64),
    }

    impl Wire for NpcWire {
        const VERSION: u64 = 1;
        const SOURCE: &'static str = "npc";

        fn to_wire(&self) -> WireRepr {
            let tags = match self {
                NpcWire::Poke(pid) => vec!["poke".into(), pid.into()],
                NpcWire::Pack(pid) => vec!["pack".into(), pid.into()],
                NpcWire::Nack(pid) => vec!["nack".into(), pid.into()],
                NpcWire::Bind(pid) => vec!["bind".into(), pid.into()],
            };
            WireRepr::new(Self::SOURCE, Self::VERSION, tags)
        }
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_npc_wire_variants() {
        let test_cases = [
            (NpcWire::Poke(123), tas!(b"poke")),
            (NpcWire::Pack(456), tas!(b"pack")),
            (NpcWire::Nack(789), tas!(b"nack")),
            (NpcWire::Bind(101), tas!(b"bind")),
        ];
        let wires = test_cases
            .iter()
            .map(|(wire, _)| wire.to_wire())
            .collect::<Vec<_>>();
        assert_eq!(
            wires,
            vec![
                WireRepr::new("npc", 1, vec!["poke".into(), 123u64.into()]),
                WireRepr::new("npc", 1, vec!["pack".into(), 456u64.into()]),
                WireRepr::new("npc", 1, vec!["nack".into(), 789u64.into()]),
                WireRepr::new("npc", 1, vec!["bind".into(), 101u64.into()]),
            ]
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_npc_poke_wire_format() {
        let wire = NpcWire::Poke(123).to_wire();
        let mut slab = NounSlab::new();
        let wire_noun = wire_to_noun(&mut slab, &wire);
        slab.set_root(wire_noun);
        // Check the wire format
        let root = unsafe { slab.root() };
        let cell = root.as_cell().unwrap_or_else(|err| {
            panic!(
                "Panicked with {err:?} at {}:{} (git sha: {:?})",
                file!(),
                line!(),
                option_env!("GIT_SHA")
            )
        });
        debug!("Wire: {:?}", nockvm::noun::DebugPath(&cell));
        // First should be direct it's "npc", 1, "poke", 123
        assert_eq!(
            cell.head()
                .as_direct()
                .unwrap_or_else(|err| {
                    panic!(
                        "Panicked with {err:?} at {}:{} (git sha: {:?})",
                        file!(),
                        line!(),
                        option_env!("GIT_SHA")
                    )
                })
                .as_ne_bytes(),
            make_tas(&mut slab, NpcWire::SOURCE)
                .as_noun()
                .as_direct()
                .unwrap_or_else(|err| {
                    panic!(
                        "Panicked with {err:?} at {}:{} (git sha: {:?})",
                        file!(),
                        line!(),
                        option_env!("GIT_SHA")
                    )
                })
                .as_ne_bytes()
        );

        // Test version
        let rest = cell.tail().as_cell().unwrap_or_else(|err| {
            panic!(
                "Panicked with {err:?} at {}:{} (git sha: {:?})",
                file!(),
                line!(),
                option_env!("GIT_SHA")
            )
        });
        assert_eq!(
            rest.head()
                .as_direct()
                .unwrap_or_else(|err| {
                    panic!(
                        "Panicked with {err:?} at {}:{} (git sha: {:?})",
                        file!(),
                        line!(),
                        option_env!("GIT_SHA")
                    )
                })
                .data(),
            1
        );

        // Test tag and pid
        let tag_cell = rest.tail().as_cell().unwrap_or_else(|err| {
            panic!(
                "Panicked with {err:?} at {}:{} (git sha: {:?})",
                file!(),
                line!(),
                option_env!("GIT_SHA")
            )
        });
        assert_eq!(
            tag_cell
                .head()
                .as_direct()
                .unwrap_or_else(|err| {
                    panic!(
                        "Panicked with {err:?} at {}:{} (git sha: {:?})",
                        file!(),
                        line!(),
                        option_env!("GIT_SHA")
                    )
                })
                .data(),
            tas!(b"poke")
        );

        let pid_cell = tag_cell.tail().as_cell().unwrap_or_else(|err| {
            panic!(
                "Panicked with {err:?} at {}:{} (git sha: {:?})",
                file!(),
                line!(),
                option_env!("GIT_SHA")
            )
        });
        assert_eq!(
            pid_cell
                .head()
                .as_direct()
                .unwrap_or_else(|err| panic!(
                    "Panicked with {err:?} at {}:{} (git sha: {:?})",
                    file!(),
                    line!(),
                    option_env!("GIT_SHA")
                ))
                .data(),
            123
        );
    }
}
