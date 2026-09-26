//! Safe conflict classification shared by storage, Restate, and the HTTP API.

use crate::pipeline::PipelineExecutionError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Conflict {
    #[error("Workflow identifier is already linked to another PDF")]
    WorkflowPdf,
    #[error("A record with this identity already exists")]
    Record,
    #[error("Document data conflicts with an existing unique identity")]
    CanonicalIdentity,
    #[error("Document contains duplicate passage identities")]
    PassageIdentity,
    #[error("Workflow has already been submitted")]
    Submission,
}

impl Conflict {
    pub fn terminal(self) -> restate_sdk::prelude::TerminalError {
        restate_sdk::prelude::TerminalError::new_with_code(409, self.to_string())
    }

    pub fn into_io(self) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::AlreadyExists, self)
    }

    pub fn from_message(message: &str) -> Option<Self> {
        [
            Self::WorkflowPdf,
            Self::Record,
            Self::CanonicalIdentity,
            Self::PassageIdentity,
            Self::Submission,
        ]
        .into_iter()
        .find(|conflict| conflict.to_string() == message)
    }
}

pub(crate) fn classify(error: &eros::ErrorUnion) -> Option<Conflict> {
    if let Some(error) = error.downcast_inner_ref::<std::io::Error>() {
        if let Some(conflict) = error
            .get_ref()
            .and_then(|error| error.downcast_ref::<Conflict>())
        {
            return Some(*conflict);
        }
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Some(Conflict::WorkflowPdf);
        }
    }
    if error
        .downcast_inner_ref::<sqlx::Error>()
        .and_then(sqlx::Error::as_database_error)
        .is_some_and(|error| error.is_unique_violation())
    {
        return Some(Conflict::Record);
    }
    if let Some(error) = error.downcast_inner_ref::<typedb_driver::Error>() {
        if typedb_duplicate(&error.code(), &error.message()) {
            return Some(Conflict::CanonicalIdentity);
        }
    }
    None
}

fn typedb_duplicate(code: &str, message: &str) -> bool {
    // TypeDB 3.12.2: key uniqueness and unique ownership violations.
    // https://github.com/typedb/typedb/blob/3.12.2/concept/thing/thing_manager/validation/mod.rs
    // The driver exposes nested server causes only as formatted stack frames.
    matches!(code, "DVL9" | "DVL13")
        || message.lines().any(|line| {
            let frame = line.trim().strip_prefix("Caused: ").unwrap_or(line.trim());
            frame.starts_with("[DVL9]") || frame.starts_with("[DVL13]")
        })
}

pub(crate) fn pipeline_io_error(error: PipelineExecutionError) -> std::io::Error {
    let source = error.into_source();
    match classify(&source) {
        Some(conflict) => conflict.into_io(),
        None => std::io::Error::other(source.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to local PostgreSQL"]
    async fn postgres_unique_violation_is_classified_as_a_conflict() {
        let pool = sqlx::PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("CREATE TEMP TABLE scepa_conflict_test (id INT PRIMARY KEY) ON COMMIT DROP")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("INSERT INTO scepa_conflict_test VALUES (1)")
            .execute(&mut *transaction)
            .await
            .unwrap();
        let error = sqlx::query("INSERT INTO scepa_conflict_test VALUES (1)")
            .execute(&mut *transaction)
            .await
            .unwrap_err();
        transaction.rollback().await.unwrap();
        assert_eq!(classify(&error.into()), Some(Conflict::Record));
    }

    #[test]
    fn only_duplicate_typedb_codes_are_conflicts() {
        assert!(typedb_duplicate("DVL9", ""));
        assert!(typedb_duplicate("DVL13", ""));
        assert!(typedb_duplicate(
            "COW5",
            "[COW5] write failed\nCaused: [DVL13] duplicate"
        ));
        assert!(!typedb_duplicate("DVL8", "key cardinality violation"));
        assert!(!typedb_duplicate("COW5", "user value contains [DVL13]"));
    }

    #[test]
    fn workflow_conflicts_are_safe_and_terminal() {
        let error: eros::ErrorUnion =
            std::io::Error::new(std::io::ErrorKind::AlreadyExists, "private details").into();
        let conflict = classify(&error).unwrap();
        assert_eq!(conflict, Conflict::WorkflowPdf);
        assert_eq!(conflict.terminal().code(), 409);
        assert!(!conflict.to_string().contains("private"));
        assert!(classify(&std::io::Error::other("duplicate").into()).is_none());
    }
}
