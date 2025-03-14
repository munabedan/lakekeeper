use http::StatusCode;
use iceberg_ext::catalog::rest::{ErrorModel, IcebergErrorResponse};
use openfga_rs::{
    tonic::{self, metadata::errors::InvalidMetadataValue, Code},
    CheckRequest, ReadRequest, ReadRequestTupleKey, WriteRequest,
};

use crate::service::authz::implementations::FgaType;

pub type OpenFGAResult<T> = Result<T, OpenFGAError>;

#[derive(Debug, thiserror::Error)]
pub enum OpenFGAError {
    #[error("Authorization Model ID failed: {reason}")]
    AuthorizationModelIdFailed { reason: String },
    #[error("Client Credential refresh failed")]
    ClientCredentialFailed(#[from] middle::Error),
    #[error("Connection to OpenFGA failed")]
    ConnectionFailed(#[from] tonic::transport::Error),
    #[error("Internal Authorization Error")]
    Internal(tonic::Status),
    #[error("Invalid Bearer Token for OpenFGA: {0}")]
    InvalidBearerToken(InvalidMetadataValue),
    #[error("Listing authentication models failed")]
    ListAuthenticationModelsFailed(tonic::Status),
    #[error("OpenFGA Error: Listing stores failed")]
    ListStoresFailed(tonic::Status),
    #[error("Reading tuples failed")]
    ReadFailed {
        read_request: Box<ReadRequest>,
        source: tonic::Status,
    },
    #[error("Authorization check failed")]
    CheckFailed {
        check_request: Box<CheckRequest>,
        source: tonic::Status,
    },
    #[error("Store creation failed: {0}")]
    StoreCreationFailed(tonic::Status),
    #[error("Store {store} not found. Please ensure to run migration first.")]
    StoreNotFound { store: String },
    #[error("Too many authorization models in database. Max allowed pages: {0}")]
    TooManyAuthorizationModels(u32),
    #[error("Too many pages")]
    TooManyPages {
        max_pages: u32,
        tuple: ReadRequestTupleKey,
    },
    #[error("Authentication to Authorization system failed")]
    Unauthenticated(tonic::Status),
    #[error("Unexpected entity for type {type:?}: {value}")]
    UnexpectedEntity { r#type: Vec<FgaType>, value: String },
    #[error("Unknown type: {0}")]
    UnknownType(String),
    #[error("Invalid entity string: `{0}`")]
    InvalidEntity(String),
    #[error("Unknown model version currently applied")]
    UnknownModelVersionApplied(u64),
    #[error("Failed to write Authorization model: {0}")]
    WriteAuthorizationModelFailed(tonic::Status),
    #[error("Failed to write Authorization tuples")]
    WriteFailed {
        write_request: WriteRequest,
        source: tonic::Status,
    },
    #[error("Too many writes and deletes in single Authorization transaction (actual) {actual} > {max} (max)")]
    TooManyWrites { actual: i32, max: i32 },
    #[error("Project ID could not be inferred from request. Please specify it explicitly.")]
    NoProjectId,
    #[error("Authentication required")]
    AuthenticationRequired,
    #[error("Unauthorized for action `{relation}` on `{object}` for `{user}`")]
    Unauthorized {
        user: String,
        relation: String,
        object: String,
    },
    #[error("Cannot assign {0} to itself")]
    SelfAssignment(String),
}

impl OpenFGAError {
    fn known_status(status: &tonic::Status) -> Option<Self> {
        match status.code() {
            Code::Unauthenticated => Some(OpenFGAError::Unauthenticated(status.clone())),
            Code::Internal => Some(OpenFGAError::Internal(status.clone())),
            _ => None,
        }
    }

    pub(crate) fn store_creation(status: tonic::Status) -> Self {
        Self::known_status(&status).unwrap_or(OpenFGAError::StoreCreationFailed(status))
    }

    pub(crate) fn list_stores(status: tonic::Status) -> Self {
        Self::known_status(&status).unwrap_or(OpenFGAError::ListStoresFailed(status))
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn list_authentication_models(status: tonic::Status) -> Self {
        Self::known_status(&status).unwrap_or(OpenFGAError::ListAuthenticationModelsFailed(status))
    }

    pub(crate) fn write_authorization_model(status: tonic::Status) -> Self {
        Self::known_status(&status).unwrap_or(OpenFGAError::WriteAuthorizationModelFailed(status))
    }

    pub(crate) fn unexpected_entity(r#type: Vec<FgaType>, value: String) -> Self {
        OpenFGAError::UnexpectedEntity { r#type, value }
    }

    fn as_status(&self) -> Option<&tonic::Status> {
        match self {
            OpenFGAError::CheckFailed { source, .. }
            | OpenFGAError::ReadFailed { source, .. }
            | OpenFGAError::WriteFailed { source, .. } => Some(source),
            OpenFGAError::Unauthenticated(status)
            | OpenFGAError::Internal(status)
            | OpenFGAError::WriteAuthorizationModelFailed(status)
            | OpenFGAError::ListStoresFailed(status)
            | OpenFGAError::StoreCreationFailed(status) => Some(status),
            _ => None,
        }
    }
}

impl From<OpenFGAError> for ErrorModel {
    fn from(err: OpenFGAError) -> Self {
        let err_msg = err.to_string();
        let status_msg = err.as_status().map(|s| s.message().to_string());
        match err {
            e @ OpenFGAError::NoProjectId => {
                ErrorModel::bad_request(err_msg, "NoProjectId", Some(Box::new(e)))
            }
            e @ OpenFGAError::AuthenticationRequired => {
                ErrorModel::unauthorized(err_msg, "AuthenticationRequired", Some(Box::new(e)))
            }
            e @ OpenFGAError::Unauthorized { .. } => {
                ErrorModel::unauthorized(err_msg, "Unauthorized", Some(Box::new(e)))
            }
            e @ OpenFGAError::WriteFailed { .. } => {
                if status_msg
                    .as_deref()
                    .is_some_and(|s| s.starts_with("cannot write a tuple which already exists"))
                {
                    ErrorModel::conflict(err_msg, "TupleAlreadyExistsError", Some(Box::new(e)))
                } else if status_msg
                    .is_some_and(|s| s.starts_with("cannot delete a tuple which does not exist"))
                {
                    ErrorModel::not_found(err_msg, "TupleNotFoundError", Some(Box::new(e)))
                } else {
                    ErrorModel::new(
                        err_msg,
                        "AuthorizationError",
                        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                        Some(Box::new(e)),
                    )
                }
            }
            _ => ErrorModel::new(
                err.to_string(),
                "AuthorizationError",
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                Some(Box::new(err)),
            ),
        }
    }
}

impl From<OpenFGAError> for IcebergErrorResponse {
    fn from(err: OpenFGAError) -> Self {
        let err_model = ErrorModel::from(err);
        err_model.into()
    }
}
