use super::error::DomainError;
use super::habit::Habit;
use super::time::TimeWindow;
use super::trigger::Trigger;

/// How a rotation's active window is determined (design spec §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationWindow {
    /// The rotation has its own start/end, independent of the global window.
    Own(TimeWindow),
    /// The rotation borrows the global day window from config.
    InheritGlobal,
    /// The rotation has no window — it is active around the clock.
    AlwaysOn,
}

/// A rotation: interval + window + members (design spec §4.3). A lone
/// interval habit is simply a rotation of one.
#[derive(Debug, Clone, PartialEq)]
pub struct Rotation {
    pub name: String,
    pub interval_secs: u32,
    pub window: RotationWindow,
    pub members: Vec<Habit>,
}

impl Rotation {
    /// Builds a rotation, rejecting a zero interval, an empty member list,
    /// and any member whose trigger isn't `RotationMember`.
    pub fn new(
        name: String,
        interval_secs: u32,
        window: RotationWindow,
        members: Vec<Habit>,
    ) -> Result<Self, DomainError> {
        if interval_secs == 0 {
            return Err(DomainError::ZeroInterval);
        }
        if members.is_empty() {
            return Err(DomainError::EmptyRotationMembers);
        }
        for member in &members {
            if !matches!(member.trigger, Trigger::RotationMember { .. }) {
                return Err(DomainError::NonRotationMemberInRotation(
                    member.name.clone(),
                ));
            }
        }
        Ok(Self {
            name,
            interval_secs,
            window,
            members,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::time::TimeOfDay;
    use crate::store::Category;

    fn sample_member(name: &str, weight: u32) -> Habit {
        Habit::new(
            name.to_string(),
            "instructions".to_string(),
            None,
            Category::Exercise,
            true,
            Trigger::rotation_member(weight).expect("valid weight"),
        )
        .expect("valid habit")
    }

    #[test]
    fn a_rotation_with_a_zero_interval_is_rejected() {
        // Given an interval of zero seconds
        // When building a rotation
        let result = Rotation::new(
            "Movement snacks".to_string(),
            0,
            RotationWindow::InheritGlobal,
            vec![sample_member("Lunge-and-reach", 2)],
        );

        // Then it fails loudly rather than accepting a rotation that never ticks
        assert!(matches!(result, Err(DomainError::ZeroInterval)));
    }

    #[test]
    fn a_rotation_with_no_members_is_rejected() {
        // Given an empty member list
        // When building a rotation
        let result = Rotation::new(
            "Movement snacks".to_string(),
            1_800,
            RotationWindow::InheritGlobal,
            vec![],
        );

        // Then it fails loudly rather than accepting a rotation with nothing to show
        assert!(matches!(result, Err(DomainError::EmptyRotationMembers)));
    }

    #[test]
    fn a_rotation_of_one_is_valid() {
        // Given a single member — "a lone interval habit is simply a rotation of one" (§4.3)
        let member = sample_member("Lunge-and-reach", 2);

        // When building a rotation from it
        let rotation = Rotation::new(
            "Movement snacks".to_string(),
            1_800,
            RotationWindow::InheritGlobal,
            vec![member.clone()],
        )
        .expect("valid rotation");

        // Then it is accepted with exactly one member
        assert_eq!(rotation.members, vec![member]);
    }

    #[test]
    fn a_member_whose_trigger_is_not_rotation_member_is_rejected() {
        // Given a habit scheduled at a fixed time, not a rotation member
        let time = TimeOfDay::new(9, 0).expect("valid time");
        let scheduled_habit = Habit::new(
            "Morning stretch".to_string(),
            "Full-body stretch routine".to_string(),
            None,
            Category::General,
            true,
            Trigger::at_time(time, crate::domain::trigger::Recurrence::Daily, true)
                .expect("valid trigger"),
        )
        .expect("valid habit");

        // When adding it to a rotation's members
        let result = Rotation::new(
            "Movement snacks".to_string(),
            1_800,
            RotationWindow::InheritGlobal,
            vec![scheduled_habit],
        );

        // Then it is rejected — every rotation member must have a RotationMember trigger
        assert!(matches!(
            result,
            Err(DomainError::NonRotationMemberInRotation(name)) if name == "Morning stretch"
        ));
    }

    #[test]
    fn an_own_window_rotation_carries_its_start_and_end() {
        // Given a rotation with its own custom window
        let start = TimeOfDay::new(7, 0).expect("valid time");
        let end = TimeOfDay::new(8, 30).expect("valid time");
        let window = TimeWindow::new(start, end).expect("valid window");

        // When building a rotation with that window
        let rotation = Rotation::new(
            "Early birds".to_string(),
            600,
            RotationWindow::Own(window),
            vec![sample_member("Wall sit", 1)],
        )
        .expect("valid rotation");

        // Then its window bounds are preserved
        assert_eq!(rotation.window, RotationWindow::Own(window));
    }
}
