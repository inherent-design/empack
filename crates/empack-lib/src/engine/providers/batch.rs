//! Exact imported inventories use one catalog allowance, never a successful subset.
use super::*;
use crate::engine::resources::AdmissionPermit;
use std::collections::BTreeMap;

pub struct ExactBatch {
    records: BTreeMap<ResolvedPin, RetainedOutput<ProviderResolution>>,
    _index: AdmissionPermit,
}
impl ExactBatch {
    pub fn records(&self) -> &BTreeMap<ResolvedPin, RetainedOutput<ProviderResolution>> {
        &self.records
    }
}
impl ProviderCatalog {
    /// Preserve every exact file declaration. Duplicate pins reuse one record; two versions
    /// of one project are a conflict, not permission to replace an earlier requested selection.
    pub async fn resolve_exact_batch(
        &self,
        scope: &mut WorkScope,
        pins: &[ResolvedPin],
        maximum: usize,
        limits: CatalogLimits,
    ) -> Result<ExactBatch> {
        ensure!(pins.len() <= maximum, CatalogError::Limit);
        let index = scope.reserve_storage(ResourceRequest {
            memory_bytes: (pins.len() as u64)
                .checked_mul(1024)
                .ok_or(CatalogError::Limit)?,
            ..Default::default()
        })?;
        let mut owners = BTreeMap::new();
        for pin in pins {
            scope.cancellation().check()?;
            pin.validate()?;
            if let Some(previous) = owners.insert(&pin.project, &pin.selection) {
                ensure!(
                    previous == &pin.selection,
                    "Import selects conflicting project pins"
                );
            }
        }
        let mut budget = transport::RequestBudget::new(limits)?;
        let mut records = BTreeMap::new();
        for pin in pins {
            scope.cancellation().check()?;
            budget.check_deadline()?;
            if records.contains_key(pin) {
                continue;
            }
            let (record, next) = self
                .resolve_exact_budget(scope, pin.clone(), limits, budget)
                .await?;
            budget = next;
            records.insert(pin.clone(), record);
        }
        scope.cancellation().check()?;
        budget.check_deadline()?;
        Ok(ExactBatch {
            records,
            _index: index,
        })
    }
}
