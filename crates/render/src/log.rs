//! The apps' `--log` option.

use bevy::log::tracing_subscriber::filter::Directive;
use bevy::log::{DEFAULT_FILTER, LogPlugin};

/// A [`LogPlugin`] that also applies `filter`, in `RUST_LOG`'s syntax, after
/// Bevy's own defaults so it can loosen or tighten them; an error names the
/// first directive that doesn't parse.
pub fn plugin(filter: Option<&str>) -> Result<LogPlugin, String> {
    let Some(filter) = filter else {
        return Ok(LogPlugin::default());
    };
    for directive in filter.split(',').filter(|d| !d.is_empty()) {
        directive
            .parse::<Directive>()
            .map_err(|e| format!("--log {directive:?}: {e}"))?;
    }
    Ok(LogPlugin {
        filter: format!("{DEFAULT_FILTER}{filter}"),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_the_filter_to_the_defaults() {
        let filter = plugin(Some("debug,wgpu=warn")).unwrap().filter;
        assert!(filter.starts_with(DEFAULT_FILTER));
        assert!(filter.ends_with(",debug,wgpu=warn"));
        assert_eq!(plugin(None).unwrap().filter, DEFAULT_FILTER);
    }

    #[test]
    fn refuses_a_bad_level() {
        assert!(
            plugin(Some("aaa_sim=loud"))
                .err()
                .unwrap()
                .contains("aaa_sim=loud")
        );
    }
}
