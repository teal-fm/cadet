use anyhow::anyhow;
use async_trait::async_trait;
use rocketman::{ingestion::LexiconIngestor, types::event::Event};
use serde_json::Value;
use sqlx::{types::Uuid, PgPool};
use time::{format_description::well_known, OffsetDateTime};
use types::types::string::Datetime;

pub struct PlayIngestor {
    sql: PgPool,
}

impl PlayIngestor {
    pub fn new(sql: PgPool) -> Self {
        Self { sql }
    }

    pub async fn insert_play(
        &self,
        play_record: &types::fm::teal::alpha::feed::play::RecordData,
        uri: &str,
        cid: &str,
        did: &str,
        rkey: &str,
    ) -> anyhow::Result<()> {
        let mut parsed_artists: Vec<(Uuid, String)> = vec![];
        for artist in &play_record.artist_names {
            // Assuming artist_mbid is optional, handle missing mbid gracefully
            let artist_mbid = if let Some(ref mbid) = play_record.artist_mb_ids {
                mbid.get(
                    play_record
                        .artist_names
                        .iter()
                        .position(|name| name == artist)
                        .unwrap_or(0),
                )
            } else {
                // Handle case where artist MBID is missing, maybe log a warning
                eprintln!("Warning: Artist MBID missing for '{}'", artist);
                continue;
            };

            let artist_name = artist.clone(); // Clone to move into the query
            if let Some(artist_mbid) = artist_mbid {
                let artist_uuid = Uuid::parse_str(artist_mbid)?;
                let res = sqlx::query!(
                    r#"
                    INSERT INTO artists (mbid, name) VALUES ($1, $2)
                    ON CONFLICT (mbid) DO NOTHING
                    RETURNING mbid;
                "#,
                    artist_uuid,
                    artist_name
                )
                .fetch_all(&self.sql)
                .await?;

                if res.len() > 0 {
                    // TODO: send request to async scrape data from local MB instance
                }

                parsed_artists.push((artist_uuid, artist_name));
            }
        }

        // Insert release if missing
        let release_mbid = if let Some(release_mbid) = &play_record.release_mb_id {
            let release_name = play_record.release_name.clone(); // Clone for move
            let release_uuid = Uuid::parse_str(release_mbid).unwrap();
            let res = sqlx::query!(
                r#"
                    INSERT INTO releases (mbid, name) VALUES ($1, $2)
                    ON CONFLICT (mbid) DO NOTHING
                    RETURNING mbid;
                "#,
                release_uuid,
                release_name
            )
            .fetch_all(&self.sql)
            .await?;

            if res.len() > 0 {
                // TODO: send request to async scrape data from local MB instance
            }
            Some(release_uuid.clone())
        } else {
            None
        };

        // Insert recording if missing
        let recording_mbid = if let Some(recording_mbid) = &play_record.recording_mb_id {
            let recording_name = play_record.track_name.clone(); // Clone for move
            let recording_uuid = Uuid::parse_str(recording_mbid).unwrap();
            let res = sqlx::query!(
                r#"
                    INSERT INTO recordings (mbid, name) VALUES ($1, $2)
                    ON CONFLICT (mbid) DO NOTHING
                    RETURNING mbid;
                "#,
                recording_uuid,
                recording_name
            )
            .fetch_all(&self.sql)
            .await?;

            if res.len() > 0 {
                // TODO: send request to async scrape data from local MB instance
            }
            Some(recording_uuid.clone())
        } else {
            None
        };

        let played_time = play_record.played_time.clone().unwrap_or(Datetime::now());
        let played_time_odt =
            OffsetDateTime::parse(played_time.as_str(), &well_known::Rfc3339).unwrap();

        // Our main insert into plays
        sqlx::query!(
            r#"
                INSERT INTO plays (
                    uri, cid, did, rkey, isrc, duration, track_name, played_time,
                    processed_time, release_mbid, release_name, recording_mbid,
                    submission_client_agent, music_service_base_domain
                ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8,
                    NOW(), $9, $10, $11, $12, $13
                ) ON CONFLICT(uri) DO UPDATE SET
                    isrc = EXCLUDED.isrc,
                    duration = EXCLUDED.duration,
                    track_name = EXCLUDED.track_name,
                    played_time = EXCLUDED.played_time,
                    processed_time = EXCLUDED.processed_time,
                    release_mbid = EXCLUDED.release_mbid,
                    release_name = EXCLUDED.release_name,
                    recording_mbid = EXCLUDED.recording_mbid,
                    submission_client_agent = EXCLUDED.submission_client_agent,
                    music_service_base_domain = EXCLUDED.music_service_base_domain;
            "#,
            uri,
            cid,
            did,
            rkey,
            play_record.isrc, // Assuming ISRC is in play_record
            play_record.duration.map(|d| d as i32),
            play_record.track_name,
            played_time_odt,
            release_mbid.clone(),
            play_record.release_name,
            recording_mbid.clone(),
            play_record.submission_client_agent,
            play_record.music_service_base_domain,
        )
        .execute(&self.sql)
        .await?;

        // Insert plays into join table
        for (mbid, artist) in &parsed_artists {
            let artist_name = artist.clone(); // Clone to move into the query

            sqlx::query!(
                r#"
                        INSERT INTO play_to_artists (play_uri, artist_mbid, artist_name) VALUES
                        ($1, $2, $3)
                        ON CONFLICT (play_uri, artist_mbid) DO NOTHING;
                    "#,
                uri,
                mbid,
                artist_name
            )
            .execute(&self.sql)
            .await?;
        }

        // Refresh materialized views concurrently (if needed, consider if this should be done less frequently)
        // For now keeping it as is from the SQL script, but consider performance implications.
        sqlx::query!("REFRESH MATERIALIZED VIEW CONCURRENTLY mv_artist_play_counts;")
            .execute(&self.sql)
            .await?;
        sqlx::query!("REFRESH MATERIALIZED VIEW CONCURRENTLY mv_release_play_counts;")
            .execute(&self.sql)
            .await?;
        sqlx::query!("REFRESH MATERIALIZED VIEW CONCURRENTLY mv_recording_play_counts;")
            .execute(&self.sql)
            .await?;
        sqlx::query!("REFRESH MATERIALIZED VIEW CONCURRENTLY mv_global_play_count;")
            .execute(&self.sql)
            .await?;

        // Optionally check materialised views (consider removing in production for performance)
        // For debugging purposes, can keep for now
        if cfg!(debug_assertions) {
            // Conditionally compile debug checks
            let artist_counts = sqlx::query!("SELECT * FROM mv_artist_play_counts;")
                .fetch_all(&self.sql)
                .await?;
            dbg!("mv_artist_play_counts: {:?}", artist_counts);

            let release_counts = sqlx::query!("SELECT * FROM mv_release_play_counts;")
                .fetch_all(&self.sql)
                .await?;
            dbg!("mv_release_play_counts: {:?}", release_counts);

            let recording_counts = sqlx::query!("SELECT * FROM mv_recording_play_counts;")
                .fetch_all(&self.sql)
                .await?;
            dbg!("mv_recording_play_counts: {:?}", recording_counts);

            let global_count = sqlx::query!("SELECT * FROM mv_global_play_count;")
                .fetch_all(&self.sql)
                .await?;
            dbg!("mv_global_play_count: {:?}", global_count);
        }

        Ok(())
    }

    async fn remove_play(&self, uri: &str) -> Result<(), sqlx::Error> {
        sqlx::query!("DELETE FROM play_to_artists WHERE play_uri = $1", uri)
            .execute(&self.sql)
            .await?;
        sqlx::query!("DELETE FROM plays WHERE uri = $1", uri)
            .execute(&self.sql)
            .await?;
        Ok(())
    }
}

/// Parses an AT uri into parts:
/// did/handle, collection, rkey
// fn parse_at_parts(aturi: &str) -> (&str, Option<&str>, Option<&str>) {
//     // example: at://did:plc:k644h4rq5bjfzcetgsa6tuby/fm.teal.alpha.feed.play/3liubcmz4sy2a
//     let split: Vec<&str> = aturi.split('/').collect();
//     let did = split.get(2).unwrap_or(&"").clone();
//     let collection = split.get(4).map(|s| *s).clone();
//     let rkey = split.get(5).map(|s| *s).clone();
//     (did, collection, rkey)
// }

fn assemble_at_uri(did: &str, collection: &str, rkey: &str) -> String {
    format!("at://{did}/{collection}/{rkey}")
}

#[async_trait]
impl LexiconIngestor for PlayIngestor {
    async fn ingest(&self, message: Event<Value>) -> anyhow::Result<()> {
        if let Some(commit) = &message.commit {
            if let Some(ref record) = &commit.record {
                let play_record = serde_json::from_value::<
                    types::fm::teal::alpha::feed::play::RecordData,
                >(record.clone())?;
                if let Some(ref commit) = message.commit {
                    if let Some(ref cid) = commit.cid {
                        // TODO: verify cid
                        self.insert_play(
                            &play_record,
                            &assemble_at_uri(&message.did, &commit.collection, &commit.rkey),
                            &cid,
                            &message.did,
                            &commit.rkey,
                        )
                        .await?;
                    }
                }
            } else {
                println!("{}: Message {} deleted", message.did, commit.rkey);
                self.remove_play(&message.did).await?;
            }
        } else {
            return Err(anyhow!("Message has no commit"));
        }
        Ok(())
    }
}
