use crate::db::Db;
use crate::db::read_uuid;
use crate::domain::note::*;
use crate::error::AppResult;
use sqlx::Row;
use uuid::Uuid;

impl Db {
    pub async fn create_note(&self, account_id: Uuid, req: &CreateNote) -> AppResult<Note> {
        let id = Uuid::new_v4();
        let row = sqlx::query(
            r#"INSERT INTO notes (id, account_id, title, content, content_type, status)
               VALUES ($1, $2, $3, $4, $5, 'pending')
               RETURNING id, account_id, title, content, content_type, status, error, created_at, processed_at"#,
        )
        .bind(id)
        .bind(account_id)
        .bind(&req.title)
        .bind(&req.content)
        .bind(&req.content_type)
        .fetch_one(&self.pool)
        .await?;
        Ok(map_note(&row))
    }

    pub async fn list_notes(&self, account_id: Uuid) -> AppResult<Vec<Note>> {
        let rows = sqlx::query(
            r#"SELECT id, account_id, title, content, content_type, status, error, created_at, processed_at
               FROM notes WHERE account_id = $1 ORDER BY created_at DESC"#,
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(map_note).collect())
    }

    pub async fn get_note(&self, id: Uuid) -> AppResult<Option<Note>> {
        let row = sqlx::query(
            r#"SELECT id, account_id, title, content, content_type, status, error, created_at, processed_at
               FROM notes WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(map_note))
    }

    pub async fn mark_note_extracted(&self, id: Uuid) -> AppResult<()> {
        sqlx::query("UPDATE notes SET status = 'extracted', processed_at = datetime('now'), error = NULL WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_note_failed(&self, id: Uuid, err: &str) -> AppResult<()> {
        sqlx::query("UPDATE notes SET status = 'failed', processed_at = datetime('now'), error = $2 WHERE id = $1")
            .bind(id)
            .bind(err)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn map_note(row: &sqlx::sqlite::SqliteRow) -> Note {
    Note {
        id: read_uuid(row, "id"),
        account_id: read_uuid(row, "account_id"),
        title: row.get("title"),
        content: row.get("content"),
        content_type: row.get("content_type"),
        status: row.get("status"),
        error: row.get("error"),
        created_at: row.get("created_at"),
        processed_at: row.get("processed_at"),
    }
}
