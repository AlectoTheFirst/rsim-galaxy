use std::collections::BTreeMap;

use crate::library::Library;
use crate::world::{Arrangement, Rule, RuleSet, Shape};

/// Generates a look enum with its names in the wire form.
macro_rules! choice {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            #[default]
            $($variant),+
        }

        impl $name {
            pub const NAMES: &[&str] = &[$($text),+];

            pub fn name(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }

            /// `None` for a text that is not a name of a value.
            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $($text => Some($name::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

choice! {
    /// Projection of the camera.
    Projection { Perspective = "perspective", Isometric = "isometric" }
}

choice! {
    /// Floor plates under the open nodes, or none.
    Space { Ground = "ground", Floating = "floating" }
}

choice! {
    /// Light of the faces.
    Light { Sun = "sun", Flat = "flat" }
}

choice! {
    /// Flow lines with no glow, or with a glow around them.
    Lines { Crisp = "crisp", Glow = "glow" }
}

choice! {
    /// Colour scheme of the theme.
    Scheme { Light = "light", Dark = "dark" }
}

choice! {
    /// The width of a flow line from its magnitude, or from the count of the flows of its trunk.
    Widths { Magnitude = "magnitude", Uniform = "uniform" }
}

choice! {
    /// Scale from the magnitude of a flow to the width of its line.
    Scale { Log = "log", Linear = "linear" }
}

/// The look of a world.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub projection: Projection,
    pub space: Space,
    pub light: Light,
    pub lines: Lines,
    pub scheme: Scheme,
    pub widths: Widths,
    pub scale: Scale,
}

/// The style of a world: the rule set, the look, and the defined shapes and the materials.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub rules: RuleSet,
    pub look: Look,
    pub library: Library,
}

impl Default for Style {
    /// Rules for the kinds site, location, room, rack, device, router, switch, l3-switch,
    /// firewall, access-point, workstation, server, laptop, load-balancer, network, function,
    /// endpoint and application with labels, the default look, no defined shape and the built-in
    /// materials.
    /// The kinds device, router, switch, firewall, access-point, workstation, server, laptop and
    /// load-balancer have the shape of their name, the kind l3-switch has the layer3-switch, the
    /// kind network the cloud and the kind endpoint the workstation.
    fn default() -> Self {
        let rule = |arrangement, size, gap, margin| Rule {
            arrangement,
            shape: Shape::Box,
            size,
            gap,
            margin,
            label: true,
        };
        let kinds = [
            ("site", rule(Arrangement::Grid, [4.0, 1.0, 4.0], 1.0, 1.0)),
            (
                "location",
                rule(Arrangement::Grid, [4.0, 1.0, 4.0], 1.0, 0.75),
            ),
            ("room", rule(Arrangement::Grid, [3.0, 1.0, 3.0], 0.75, 0.5)),
            ("rack", rule(Arrangement::Stack, [1.0, 2.0, 1.0], 0.1, 1.0)),
            (
                "device",
                Rule {
                    shape: Shape::Device,
                    ..rule(Arrangement::Stack, [1.6, 0.8, 1.2], 0.5, 0.5)
                },
            ),
            (
                "router",
                Rule {
                    shape: Shape::Router,
                    ..rule(Arrangement::Stack, [1.6, 0.4, 1.6], 0.5, 0.5)
                },
            ),
            (
                "switch",
                Rule {
                    shape: Shape::Switch,
                    ..rule(Arrangement::Stack, [1.6, 0.35, 1.6], 0.5, 0.5)
                },
            ),
            (
                "l3-switch",
                Rule {
                    shape: Shape::LayerThreeSwitch,
                    ..rule(Arrangement::Stack, [1.6, 0.35, 1.6], 0.5, 0.5)
                },
            ),
            (
                "firewall",
                Rule {
                    shape: Shape::Firewall,
                    ..rule(Arrangement::Stack, [1.6, 1.2, 0.8], 0.5, 0.5)
                },
            ),
            (
                "function",
                rule(Arrangement::Row, [2.0, 0.5, 1.0], 0.5, 0.5),
            ),
            (
                "access-point",
                Rule {
                    shape: Shape::AccessPoint,
                    ..rule(Arrangement::Stack, [1.4, 0.35, 1.0], 0.5, 0.5)
                },
            ),
            (
                "workstation",
                Rule {
                    shape: Shape::Workstation,
                    ..rule(Arrangement::Row, [1.1, 1.0, 0.8], 0.5, 0.5)
                },
            ),
            (
                "server",
                Rule {
                    shape: Shape::Server,
                    ..rule(Arrangement::Stack, [0.8, 1.6, 1.0], 0.5, 0.5)
                },
            ),
            (
                "laptop",
                Rule {
                    shape: Shape::Laptop,
                    ..rule(Arrangement::Row, [1.2, 0.8, 1.0], 0.5, 0.5)
                },
            ),
            (
                "load-balancer",
                Rule {
                    shape: Shape::LoadBalancer,
                    ..rule(Arrangement::Stack, [1.6, 0.4, 1.2], 0.5, 0.5)
                },
            ),
            (
                "network",
                Rule {
                    shape: Shape::Cloud,
                    ..rule(Arrangement::Row, [1.8, 1.1, 0.7], 0.5, 0.5)
                },
            ),
            (
                "endpoint",
                Rule {
                    shape: Shape::Workstation,
                    ..rule(Arrangement::Row, [1.1, 1.0, 0.8], 0.5, 0.5)
                },
            ),
            (
                "application",
                rule(Arrangement::Row, [1.5, 1.5, 1.5], 0.5, 0.5),
            ),
        ];
        Self {
            rules: RuleSet {
                root: rule(Arrangement::Row, [0.0; 3], 2.0, 2.0),
                default: rule(Arrangement::Row, [1.0, 1.0, 1.0], 0.5, 0.5),
                kinds: kinds
                    .into_iter()
                    .map(|(kind, rule)| (kind.to_owned(), rule))
                    .collect::<BTreeMap<_, _>>(),
            },
            look: Look::default(),
            library: Library::default(),
        }
    }
}
