use crate::services::derived_artifacts::DerivedArtifactDeps;

use super::super::creation::context::QueryJobsDeps;

pub(super) fn derived_artifact_deps<'a>(deps: &'a QueryJobsDeps<'a>) -> DerivedArtifactDeps<'a> {
    DerivedArtifactDeps::new(deps.replay.render_rs_bin)
}
