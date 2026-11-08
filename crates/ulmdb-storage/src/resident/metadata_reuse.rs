//! Prepared owns the existing private-cache lease until all new-owner rows clear.
use super::super::private_workspace;
use super::NewVersion;
use std::ops::Deref;
pub(super) struct FrozenVersions {
    rows: Vec<NewVersion>,
    workspace: Option<private_workspace::Pages>,
}
impl FrozenVersions {
    pub(super) fn cached(mut workspace: private_workspace::Pages) -> Self {
        Self {
            rows: workspace.take_sealed(),
            workspace: Some(workspace),
        }
    }
}
impl From<Vec<NewVersion>> for FrozenVersions {
    fn from(rows: Vec<NewVersion>) -> Self {
        Self {
            rows,
            workspace: None,
        }
    }
}
impl Deref for FrozenVersions {
    type Target = [NewVersion];
    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}
impl<'a> IntoIterator for &'a FrozenVersions {
    type Item = &'a NewVersion;
    type IntoIter = std::slice::Iter<'a, NewVersion>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}
impl Drop for FrozenVersions {
    fn drop(&mut self) {
        // Clear owners before returning buffers or taking the existing cache lock.
        self.rows.clear();
        if let Some(mut workspace) = self.workspace.take() {
            workspace.restore_sealed(std::mem::take(&mut self.rows));
            // Pages::drop runs the existing quota/epoch-checked return path.
        }
    }
}
