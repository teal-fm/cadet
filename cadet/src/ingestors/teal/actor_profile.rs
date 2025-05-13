use async_trait::async_trait;
use atrium_api::types::string::{Datetime, Did};
use multibase::Base;
use rocketman::{ingestion::LexiconIngestor, types::event::Event};
use serde_json::Value;
use sqlx::PgPool;
use time::{format_description::well_known, OffsetDateTime};

use crate::resolve::resolve_identity;

pub struct ActorProfileIngestor {
    sql: PgPool,
}

fn get_blob_ref(blob_ref: &atrium_api::types::BlobRef) -> anyhow::Result<String> {
    match blob_ref {
        atrium_api::types::BlobRef::Typed(r) => match r {
            atrium_api::types::TypedBlobRef::Blob(blob) => {
                // Use into_v1() to get the CID v1, then convert to Base32 (bafy...)
                blob.r#ref
                    .0
                    .to_string_of_base(Base::Base32Lower)
                    .map_err(|e| anyhow::anyhow!(e))
            }
        },
        atrium_api::types::BlobRef::Untyped(_) => {
            Err(anyhow::anyhow!("Untyped blob reference not supported"))
        }
    }
}

impl ActorProfileIngestor {
    pub fn new(sql: PgPool) -> Self {
        Self { sql }
    }

    pub async fn insert_profile(
        &self,
        provided_did: Did,
        profile: &types::fm::teal::alpha::actor::profile::RecordData,
    ) -> anyhow::Result<()> {
        // TODO: cache the doc for like 8 hours or something
        let did = resolve_identity(provided_did.as_str(), "https://public.api.bsky.app").await?;

        let handle = did.doc.also_known_as.first().to_owned();

        let created_time = profile.created_at.clone().unwrap_or(Datetime::now());
        let created_time_odt =
            OffsetDateTime::parse(created_time.as_str(), &well_known::Rfc3339).unwrap();

        dbg!(&profile.avatar);
        let avatar = profile
            .avatar
            .clone()
            .and_then(|bref| get_blob_ref(&bref).ok());
        let banner = profile
            .banner
            .clone()
            .and_then(|bref| get_blob_ref(&bref).ok());
        sqlx::query!(
            r#"
                INSERT INTO profiles (did, handle, display_name, description, description_facets, avatar, banner, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            did.identity,
            handle,
            profile.display_name,
            profile.description,
            serde_json::to_value(profile.description_facets.clone())?,
            avatar,
            banner,
            created_time_odt
        )
        .execute(&self.sql)
        .await?;
        Ok(())
    }
    pub async fn remove_profile(&self, did: Did) -> anyhow::Result<()> {
        sqlx::query!(
            r#"
                DELETE FROM profiles WHERE did = $1
            "#,
            did.as_str()
        )
        .execute(&self.sql)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl LexiconIngestor for ActorProfileIngestor {
    async fn ingest(&self, message: Event<Value>) -> anyhow::Result<()> {
        if let Some(commit) = &message.commit {
            if let Some(ref record) = &commit.record {
                let record = serde_json::from_value::<
                    types::fm::teal::alpha::actor::profile::RecordData,
                >(record.clone())?;
                if let Some(ref commit) = message.commit {
                    if let Some(ref _cid) = commit.cid {
                        // TODO: verify cid
                        self.insert_profile(
                            Did::new(message.did)
                                .map_err(|e| anyhow::anyhow!("Failed to create Did: {}", e))?,
                            &record,
                        )
                        .await?;
                    }
                }
            } else {
                println!("{}: Message {} deleted", message.did, commit.rkey);
                self.remove_profile(
                    Did::new(message.did)
                        .map_err(|e| anyhow::anyhow!("Failed to create Did: {}", e))?,
                )
                .await?;
            }
        } else {
            return Err(anyhow::anyhow!("Message has no commit"));
        }
        Ok(())
    }
}
