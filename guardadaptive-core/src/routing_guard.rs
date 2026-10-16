//! v0.31 routing safety gate for a FUTURE explicit virtual controller.
//! It does not detect actual Windows processes or install HID filters.
//! Game output remains denied by the shipped backend regardless of verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    NoConsent,
    UnboundPhysicalId,
    NoPhysicalInput,
    NoVirtualBackend,
    DuplicateGameVisibleController,
    SlotCollision,
    UnverifiedVisibility,
    SimulatedTopologyEligible,
}
impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Self::NoConsent => "BLOCKED_NO_CONSENT",
            Self::UnboundPhysicalId => "BLOCKED_UNBOUND_HARDWARE",
            Self::NoPhysicalInput => "BLOCKED_MISSING_PHYSICAL_INPUT",
            Self::NoVirtualBackend => "BLOCKED_NO_VIRTUAL_BACKEND",
            Self::DuplicateGameVisibleController => "BLOCKED_DUPLICATE_GAME_INPUT",
            Self::SlotCollision => "BLOCKED_SLOT_COLLISION",
            Self::UnverifiedVisibility => "BLOCKED_TOPOLOGY_UNKNOWN",
            Self::SimulatedTopologyEligible => "SIMULATED_TOPOLOGY_ONLY_NO_DEVICE_WRITES",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topology {
    pub explicit_user_consent: bool,
    pub stable_hardware_identity_authenticated: bool,
    pub physical_input_slot: Option<u8>,
    pub virtual_output_slot: Option<u8>,
    pub virtual_backend_verified: bool,
    pub physical_visible_to_game: Option<bool>,
    pub virtual_visible_to_game: Option<bool>,
}
impl Topology {
    pub fn check(self) -> Verdict {
        if !self.explicit_user_consent {
            return Verdict::NoConsent;
        }
        if !self.stable_hardware_identity_authenticated {
            return Verdict::UnboundPhysicalId;
        }
        let Some(physical) = self.physical_input_slot else {
            return Verdict::NoPhysicalInput;
        };
        let Some(virtual_slot) = self.virtual_output_slot else {
            return Verdict::NoVirtualBackend;
        };
        if physical > 3 || virtual_slot > 3 {
            return Verdict::SlotCollision;
        }
        if !self.virtual_backend_verified {
            return Verdict::NoVirtualBackend;
        }
        if physical == virtual_slot {
            return Verdict::SlotCollision;
        }
        match (self.physical_visible_to_game, self.virtual_visible_to_game) {
            (Some(true), Some(true)) => Verdict::DuplicateGameVisibleController,
            (Some(false), Some(true)) => Verdict::SimulatedTopologyEligible,
            _ => Verdict::UnverifiedVisibility,
        }
    }
}
pub const ACTUAL_HARDWARE_ROUTE_AVAILABLE: bool = false;
pub fn physical_output_allowed(_verdict: Verdict) -> bool {
    // Hard stop until a separate driver-bearing release is validated.
    false
}
pub fn report() -> String {
    let topology = Topology {
        explicit_user_consent: false,
        stable_hardware_identity_authenticated: false,
        physical_input_slot: None,
        virtual_output_slot: None,
        virtual_backend_verified: false,
        physical_visible_to_game: None,
        virtual_visible_to_game: None,
    };
    format!(
        concat!(
            "{{\"schema\":1,\"source\":\"simulated_controller_routing_guard\",",
            "\"default_verdict\":\"{}\",",
            "\"physical_output_allowed\":{},",
            "\"driver_available\":false,",
            "\"physical_device_identity_authenticated\":false,",
            "\"real_game_visibility_measured\":false,",
            "\"production_qualified\":false}}\n"
        ),
        topology.check().name(),
        physical_output_allowed(topology.check())
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn good() -> Topology {
        Topology {
            explicit_user_consent: true,
            stable_hardware_identity_authenticated: true,
            physical_input_slot: Some(0),
            virtual_output_slot: Some(1),
            virtual_backend_verified: true,
            physical_visible_to_game: Some(false),
            virtual_visible_to_game: Some(true),
        }
    }
    #[test]
    fn every_unsafe_path_is_denied() {
        let g = good();
        assert_eq!(g.check(), Verdict::SimulatedTopologyEligible);
        assert!(!physical_output_allowed(g.check()));
        assert_eq!(
            Topology {
                explicit_user_consent: false,
                ..g
            }
            .check(),
            Verdict::NoConsent
        );
        assert_eq!(
            Topology {
                stable_hardware_identity_authenticated: false,
                ..g
            }
            .check(),
            Verdict::UnboundPhysicalId
        );
        assert_eq!(
            Topology {
                physical_input_slot: None,
                ..g
            }
            .check(),
            Verdict::NoPhysicalInput
        );
        assert_eq!(
            Topology {
                virtual_output_slot: None,
                ..g
            }
            .check(),
            Verdict::NoVirtualBackend
        );
        assert_eq!(
            Topology {
                virtual_backend_verified: false,
                ..g
            }
            .check(),
            Verdict::NoVirtualBackend
        );
        assert_eq!(
            Topology {
                virtual_output_slot: Some(0),
                ..g
            }
            .check(),
            Verdict::SlotCollision
        );
        assert_eq!(
            Topology {
                physical_visible_to_game: Some(true),
                ..g
            }
            .check(),
            Verdict::DuplicateGameVisibleController
        );
        assert_eq!(
            Topology {
                physical_visible_to_game: None,
                ..g
            }
            .check(),
            Verdict::UnverifiedVisibility
        );
        assert_eq!(
            Topology {
                virtual_visible_to_game: Some(false),
                ..g
            }
            .check(),
            Verdict::UnverifiedVisibility
        );
    }
    #[test]
    fn actual_device_output_is_unavailable_even_if_topology_seems_eligible() {
        const { assert!(!ACTUAL_HARDWARE_ROUTE_AVAILABLE) };
        for physical in [None, Some(0), Some(1), Some(2), Some(3), Some(9)] {
            for virtual_output in [None, Some(0), Some(1), Some(2), Some(3), Some(9)] {
                let t = Topology {
                    physical_input_slot: physical,
                    virtual_output_slot: virtual_output,
                    ..good()
                };
                assert!(!physical_output_allowed(t.check()));
            }
        }
        assert!(report().contains("\"physical_output_allowed\":false"));
    }
}
