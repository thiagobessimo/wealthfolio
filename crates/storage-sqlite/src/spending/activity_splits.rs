//! Storage adapter for spending::activity_splits.

use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::{get_connection, DbPool, WriteHandle};
use crate::errors::StorageError;
use crate::schema::{spending_activity_splits, taxonomy_categories};
use crate::spending::activity_sync::should_sync_activity_local_id_outbox;
use wealthfolio_core::sync::SyncEntity;
use wealthfolio_spending::activity_splits::{
    ActivitySplit, ActivitySplitRepositoryTrait, NewActivitySplit,
};

#[derive(Queryable, Identifiable, Selectable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::spending_activity_splits)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
#[serde(rename_all = "camelCase")]
pub struct ActivitySplitDB {
    pub id: String,
    pub activity_id: String,
    pub taxonomy_id: String,
    pub category_id: String,
    pub amount: String,
    pub note: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Insertable, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = crate::schema::spending_activity_splits)]
pub struct NewActivitySplitDB {
    pub id: String,
    pub activity_id: String,
    pub taxonomy_id: String,
    pub category_id: String,
    pub amount: String,
    pub note: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

impl crate::sync::SyncOutboxModel for ActivitySplitDB {
    const ENTITY: SyncEntity = SyncEntity::SpendingActivitySplit;

    fn sync_entity_id(&self) -> &str {
        &self.id
    }
}

fn parse_dt(s: &str) -> NaiveDateTime {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.naive_utc())
        .unwrap_or_else(|_| chrono::Utc::now().naive_utc())
}

impl From<ActivitySplitDB> for ActivitySplit {
    fn from(db: ActivitySplitDB) -> Self {
        Self {
            id: db.id,
            activity_id: db.activity_id,
            taxonomy_id: db.taxonomy_id,
            category_id: db.category_id,
            amount: Decimal::from_str(&db.amount).unwrap_or(Decimal::ZERO),
            note: db.note,
            sort_order: db.sort_order,
            created_at: parse_dt(&db.created_at),
            updated_at: parse_dt(&db.updated_at),
        }
    }
}

pub struct ActivitySplitRepository {
    pool: Arc<DbPool>,
    writer: WriteHandle,
}

impl ActivitySplitRepository {
    pub fn new(pool: Arc<DbPool>, writer: WriteHandle) -> Self {
        Self { pool, writer }
    }
}

#[async_trait]
impl ActivitySplitRepositoryTrait for ActivitySplitRepository {
    async fn list_for_activity(&self, activity_id: &str) -> Result<Vec<ActivitySplit>> {
        let mut conn = get_connection(&self.pool).map_err(|e| anyhow::anyhow!(e))?;
        let rows = spending_activity_splits::table
            .filter(spending_activity_splits::activity_id.eq(activity_id))
            .order((
                spending_activity_splits::sort_order.asc(),
                spending_activity_splits::created_at.asc(),
                spending_activity_splits::id.asc(),
            ))
            .load::<ActivitySplitDB>(&mut conn)
            .map_err(StorageError::from)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_for_activities(&self, activity_ids: &[String]) -> Result<Vec<ActivitySplit>> {
        if activity_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = get_connection(&self.pool).map_err(|e| anyhow::anyhow!(e))?;
        const CHUNK: usize = 500;
        let mut out = Vec::new();
        for chunk in activity_ids.chunks(CHUNK) {
            let rows = spending_activity_splits::table
                .filter(spending_activity_splits::activity_id.eq_any(chunk))
                .order((
                    spending_activity_splits::activity_id.asc(),
                    spending_activity_splits::sort_order.asc(),
                    spending_activity_splits::created_at.asc(),
                    spending_activity_splits::id.asc(),
                ))
                .load::<ActivitySplitDB>(&mut conn)
                .map_err(StorageError::from)
                .map_err(|e| anyhow::anyhow!(e))?;
            out.extend(rows.into_iter().map(ActivitySplit::from));
        }
        Ok(out)
    }

    async fn categories_belong_to_taxonomy(
        &self,
        taxonomy_id: &str,
        category_ids: &[String],
    ) -> Result<bool> {
        if category_ids.is_empty() {
            return Ok(true);
        }

        let unique = category_ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let mut conn = get_connection(&self.pool).map_err(|e| anyhow::anyhow!(e))?;
        let matched = taxonomy_categories::table
            .filter(taxonomy_categories::taxonomy_id.eq(taxonomy_id))
            .filter(taxonomy_categories::id.eq_any(unique.iter().collect::<Vec<_>>()))
            .select(taxonomy_categories::id)
            .load::<String>(&mut conn)
            .map_err(StorageError::from)
            .map_err(|e| anyhow::anyhow!(e))?
            .into_iter()
            .collect::<std::collections::HashSet<_>>();

        Ok(matched.len() == unique.len())
    }

    async fn replace_for_activity(
        &self,
        activity_id: &str,
        splits: Vec<NewActivitySplit>,
    ) -> Result<Vec<ActivitySplit>> {
        let activity_id = activity_id.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.writer
            .exec_tx(move |tx| {
                let existing_ids = spending_activity_splits::table
                    .filter(spending_activity_splits::activity_id.eq(&activity_id))
                    .select(spending_activity_splits::id)
                    .load::<String>(tx.conn())
                    .map_err(StorageError::from)?;
                let should_sync = should_sync_activity_local_id_outbox(tx.conn(), &activity_id)?;

                diesel::delete(
                    spending_activity_splits::table
                        .filter(spending_activity_splits::activity_id.eq(&activity_id)),
                )
                .execute(tx.conn())
                .map_err(StorageError::from)?;

                let rows = splits
                    .into_iter()
                    .enumerate()
                    .map(|(index, split)| NewActivitySplitDB {
                        id: Uuid::new_v4().to_string(),
                        activity_id: activity_id.clone(),
                        taxonomy_id: split.taxonomy_id,
                        category_id: split.category_id,
                        amount: split.amount.normalize().to_string(),
                        note: split.note,
                        sort_order: split.sort_order.unwrap_or(index as i32),
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    })
                    .collect::<Vec<_>>();

                let mut inserted = Vec::with_capacity(rows.len());
                for row in rows {
                    let inserted_row = diesel::insert_into(spending_activity_splits::table)
                        .values(&row)
                        .returning(ActivitySplitDB::as_returning())
                        .get_result(tx.conn())
                        .map_err(StorageError::from)?;
                    inserted.push(inserted_row);
                }

                if should_sync {
                    for id in existing_ids {
                        tx.delete::<ActivitySplitDB>(id);
                    }
                    for row in &inserted {
                        tx.insert(row)?;
                    }
                }

                Ok(inserted)
            })
            .await
            .map(|rows| rows.into_iter().map(ActivitySplit::from).collect())
            .map_err(|e| anyhow::anyhow!(e))
    }

    async fn clear_for_activity(&self, activity_id: &str) -> Result<()> {
        let activity_id = activity_id.to_string();
        self.writer
            .exec_tx(move |tx| {
                let existing_ids = spending_activity_splits::table
                    .filter(spending_activity_splits::activity_id.eq(&activity_id))
                    .select(spending_activity_splits::id)
                    .load::<String>(tx.conn())
                    .map_err(StorageError::from)?;
                let should_sync = should_sync_activity_local_id_outbox(tx.conn(), &activity_id)?;

                diesel::delete(
                    spending_activity_splits::table
                        .filter(spending_activity_splits::activity_id.eq(&activity_id)),
                )
                .execute(tx.conn())
                .map_err(StorageError::from)?;

                if should_sync {
                    for id in existing_ids {
                        tx.delete::<ActivitySplitDB>(id);
                    }
                }
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }
}
