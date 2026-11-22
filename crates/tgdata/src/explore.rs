//! The settings regiments explore by.

use crate::GameData;

/// How an exploring regiment weighs the places it might head for next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExploreRules {
    /// How many places it weighs around itself.
    pub around_itself: u32,
    /// How many places it weighs around the point it explores.
    pub around_point: u32,
    /// How near and far from it the places around itself are.
    pub radius: [f32; 2],
    /// How many tiles into unseen ground a place around the point is.
    pub into_fog: u32,
    /// What a place around itself is worth to start with.
    pub itself_value: f32,
    /// What a place around the point is worth to start with.
    pub point_value: f32,
    /// What each unit of a place's distance from the point adds.
    pub per_distance: f32,
    /// What lying the same way as its last place adds, at most.
    pub same_way: f32,
    /// What finishing off unseen ground adds.
    pub finishing: f32,
    /// The most chance adds.
    pub fuzz: f32,
    /// How near the place it's headed for it chooses the next.
    pub close_enough: f32,
}

impl Default for ExploreRules {
    fn default() -> Self {
        ExploreRules {
            around_itself: 20,
            around_point: 20,
            radius: [20.0, 30.0],
            into_fog: 8,
            itself_value: 40.0,
            point_value: 0.0,
            per_distance: -1.0,
            same_way: 20.0,
            finishing: 20.0,
            fuzz: 20.0,
            close_enough: 3.0,
        }
    }
}

impl GameData {
    pub fn explore_rules(&self) -> ExploreRules {
        let d = ExploreRules::default();
        let svars = self.svars();
        let int = |of: fn(&crate::defs::SVars) -> Option<u32>, or| {
            svars.iter().find_map(of).unwrap_or(or)
        };
        let fixed = |of: fn(&crate::defs::SVars) -> Option<f32>, or| {
            svars.iter().find_map(of).unwrap_or(or)
        };
        ExploreRules {
            around_itself: int(|v| v.explore_num_actor_locations, d.around_itself),
            around_point: int(|v| v.explore_num_waypoint_locations, d.around_point),
            radius: [
                fixed(|v| v.explore_actor_radius_min, d.radius[0]),
                fixed(|v| v.explore_actor_radius_max, d.radius[1]),
            ],
            into_fog: int(|v| v.explore_waypoint_tiles_into_fog, d.into_fog),
            itself_value: fixed(|v| v.explore_actor_base_value, d.itself_value),
            point_value: fixed(|v| v.explore_waypoint_base_value, d.point_value),
            per_distance: fixed(|v| v.explore_distance_multiplier, d.per_distance),
            same_way: fixed(|v| v.explore_dot_product_multiplier, d.same_way),
            finishing: fixed(|v| v.explore_finish_area_bonus, d.finishing),
            fuzz: fixed(|v| v.explore_fuzzy_factor, d.fuzz),
            close_enough: fixed(|v| v.explore_inertia_close_enough, d.close_enough),
        }
    }
}
