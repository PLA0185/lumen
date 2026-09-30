use crate::{
    commands::AppState,
    db::{now_stamp, Db},
    error::AppResult,
    recurrence::{Freq, RecurrenceRule},
};
use serde::Serialize;
use sqlx::FromRow;
use tauri::State;

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyArrangement {
    pub series_id: String,
    pub title: String,
    pub rrule: String,
    pub tzid: String,
    pub dtstart_local: String,
    pub has_start_time: i64,
    pub task_id: Option<String>,
    pub occurrence_key: Option<String>,
    pub open_count: i64,
    #[sqlx(default)]
    pub description: String,
}

pub async fn list_impl(db: &Db, now: &str) -> AppResult<Vec<WeeklyArrangement>> {
    let rows = sqlx::query_as::<_, WeeklyArrangement>(r#"
        WITH anchors AS (
            SELECT s.*,
                COALESCE(
                    (SELECT MIN(occurrence_key) FROM tasks WHERE series_id=s.id AND deleted_at IS NULL AND status NOT IN ('done','archived') AND occurrence_key>=?1),
                    (SELECT MAX(occurrence_key) FROM tasks WHERE series_id=s.id AND deleted_at IS NULL AND status NOT IN ('done','archived')),
                    (SELECT MAX(occurrence_key) FROM tasks WHERE series_id=s.id AND deleted_at IS NULL)
                ) AS anchor_key
            FROM task_series s
        )
        SELECT a.id AS series_id,
            COALESCE((SELECT override_title FROM task_series_segments WHERE series_id=a.id AND effective_from_occurrence<=a.anchor_key AND override_title IS NOT NULL ORDER BY rule_version DESC LIMIT 1),t.title) AS title,
            COALESCE((SELECT new_rrule FROM task_series_segments WHERE series_id=a.id AND effective_from_occurrence<=a.anchor_key AND new_rrule IS NOT NULL ORDER BY rule_version DESC LIMIT 1),a.rrule) AS rrule,
            COALESCE((SELECT new_tzid FROM task_series_segments WHERE series_id=a.id AND effective_from_occurrence<=a.anchor_key AND new_tzid IS NOT NULL ORDER BY rule_version DESC LIMIT 1),a.tzid) AS tzid,
            a.dtstart_local, a.has_start_time,
            (SELECT id FROM tasks WHERE series_id=a.id AND occurrence_key=a.anchor_key AND deleted_at IS NULL LIMIT 1) AS task_id,
            a.anchor_key AS occurrence_key,
            (SELECT COUNT(*) FROM tasks WHERE series_id=a.id AND deleted_at IS NULL AND status NOT IN ('done','archived')) AS open_count
        FROM anchors a JOIN task_series_template t ON t.series_id=a.id
        WHERE a.anchor_key IS NOT NULL
        ORDER BY title COLLATE NOCASE, a.id
    "#).bind(now).fetch_all(db.pool()).await?;
    let mut result = Vec::new();
    for mut row in rows {
        let rule = RecurrenceRule::from_rrule_string(
            &row.rrule,
            &row.tzid,
            &row.dtstart_local,
            row.has_start_time != 0,
        )?;
        if rule.freq != Freq::Weekly {
            continue;
        }
        row.description = rule.describe();
        result.push(row);
    }
    Ok(result)
}

#[tauri::command]
pub async fn recurring_weekly_list(
    state: State<'_, AppState>,
) -> AppResult<Vec<WeeklyArrangement>> {
    list_impl(&state.db, &now_stamp()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn weekly_view_groups_instances_and_uses_effective_rule_segments() {
        let dir = std::env::temp_dir().join(format!("lumen-weekly-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        sqlx::query("INSERT INTO task_series(id,rrule,tzid,dtstart_local,created_at,updated_at) VALUES ('s','FREQ=WEEKLY;BYDAY=MO,WE,FR;X-LUMEN-HOLIDAYS=CN','Asia/Shanghai','2026-09-21T09:00:00','2026-09-20','2026-09-20')").execute(db.pool()).await.unwrap();
        sqlx::query("INSERT INTO task_series_template(series_id,title) VALUES ('s','业务复盘')")
            .execute(db.pool())
            .await
            .unwrap();
        for (id, key, status) in [
            ('a', "2026-09-21T01:00:00.000Z", "done"),
            ('b', "2026-09-30T01:00:00.000Z", "todo"),
            ('c', "2026-10-02T01:00:00.000Z", "todo"),
        ] {
            sqlx::query("INSERT INTO tasks(id,title,status,series_id,occurrence_key,created_at,updated_at) VALUES (?,?,?,?,?,'2026-09-20','2026-09-20')").bind(id.to_string()).bind("业务复盘").bind(status).bind("s").bind(key).execute(db.pool()).await.unwrap();
        }
        let items = list_impl(&db, "2026-09-30T00:00:00.000Z").await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].open_count, 2);
        assert_eq!(items[0].task_id.as_deref(), Some("b"));
        assert!(
            items[0].description.contains("周一")
                && items[0].description.contains("周三")
                && items[0].description.contains("法定节假日")
        );
        sqlx::query("INSERT INTO task_series_segments(id,series_id,rule_version,effective_from_occurrence,new_rrule,created_at) VALUES ('segment','s',2,'2026-09-30T01:00:00.000Z','FREQ=DAILY','2026-09-20')").execute(db.pool()).await.unwrap();
        assert!(list_impl(&db, "2026-09-30T00:00:00.000Z")
            .await
            .unwrap()
            .is_empty());
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
