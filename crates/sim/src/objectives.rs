//! Each side's mission objectives, as the map's triggers give, settle and
//! take them away.

/// Where an objective stands for the side holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Active,
    Completed,
    Failed,
}

/// An objective a side holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Objective {
    /// The map's name for it, by which triggers refer to it.
    pub name: String,
    /// What it says, numbered as the map's author wrote it.
    pub text: String,
    pub primary: bool,
    pub standing: Standing,
}

/// The objectives a side holds, in the order they show.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Objectives(pub Vec<Objective>);

impl Objectives {
    /// Gives an objective, active: when `sorted`, ahead of the first whose
    /// text sorts the same or later, so the authors' numbering puts them in
    /// order; otherwise last. One already held is given again, and shows
    /// twice.
    pub fn give(&mut self, name: &str, text: &str, primary: bool, sorted: bool) {
        let at = (self.0.iter())
            .position(|o| sorted && o.text.as_str() >= text)
            .unwrap_or(self.0.len());
        let objective = Objective {
            name: name.to_string(),
            text: text.to_string(),
            primary,
            standing: Standing::Active,
        };
        self.0.insert(at, objective);
    }

    /// Sets where the held objective `name` stands; `None`, changing
    /// nothing, if it isn't held.
    pub fn settle(&mut self, name: &str, standing: Standing) -> Option<&Objective> {
        let objective = self.0.iter_mut().find(|o| o.name == name)?;
        objective.standing = standing;
        Some(objective)
    }

    /// Takes away the held objective `name`, the first of it if given
    /// twice.
    pub fn take(&mut self, name: &str) -> Option<Objective> {
        let at = self.0.iter().position(|o| o.name == name)?;
        Some(self.0.remove(at))
    }

    /// Whether the objective `name` is held, standing as `standing`.
    pub fn stands(&self, name: &str, standing: Standing) -> bool {
        self.0
            .iter()
            .any(|o| o.name == name && o.standing == standing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(objectives: &Objectives) -> Vec<&str> {
        objectives.0.iter().map(|o| o.text.as_str()).collect()
    }

    #[test]
    fn given_ones_go_in_order_of_their_text() {
        let mut objectives = Objectives::default();
        objectives.give("objective_8", "2. Hold Kursk.", true, true);
        objectives.give("objective_2", "3. Hold Orel.", false, true);
        objectives.give("objective_5", "1. Kursk must not fall.", true, true);
        let order = ["1. Kursk must not fall.", "2. Hold Kursk.", "3. Hold Orel."];
        assert_eq!(texts(&objectives), order);
    }

    #[test]
    fn unsorted_ones_go_in_the_order_given() {
        let mut objectives = Objectives::default();
        objectives.give("objective_8", "2. Hold Kursk.", true, false);
        objectives.give("objective_5", "1. Kursk must not fall.", true, false);
        assert_eq!(
            texts(&objectives),
            ["2. Hold Kursk.", "1. Kursk must not fall."]
        );
    }

    #[test]
    fn one_given_twice_shows_twice_and_goes_one_at_a_time() {
        let mut objectives = Objectives::default();
        objectives.give("objective_0", "1. Capture Maleme.", true, true);
        objectives.give("objective_0", "1. Capture Maleme.", true, true);
        assert_eq!(objectives.0.len(), 2);
        assert!(objectives.take("objective_0").is_some());
        assert_eq!(objectives.0.len(), 1);
    }

    #[test]
    fn settling_changes_only_what_is_held() {
        let mut objectives = Objectives::default();
        assert!(
            objectives
                .settle("objective_0", Standing::Completed)
                .is_none()
        );
        assert!(!objectives.stands("objective_0", Standing::Completed));
        assert!(!objectives.stands("objective_0", Standing::Failed));
        objectives.give("objective_0", "1. Capture Maleme.", true, true);
        objectives.settle("objective_0", Standing::Completed);
        assert!(objectives.stands("objective_0", Standing::Completed));
        objectives.settle("objective_0", Standing::Active);
        assert!(objectives.stands("objective_0", Standing::Active));
    }
}
