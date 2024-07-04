pub mod auth;
mod catalog;
pub mod config;
pub mod contract_verification;
pub mod event_publisher;
pub mod secrets;
pub mod storage;
pub mod tabular_idents;
pub mod token_verification;

pub use catalog::{
    Catalog, CommitTableResponse, CommitTableResponseExt, CommitTransactionRequest,
    CreateNamespaceRequest, CreateNamespaceResponse, CreateTableRequest, CreateTableResponse,
    GetNamespaceResponse, GetStorageConfigResponse, GetTableMetadataResponse, GetWarehouseResponse,
    ListNamespacesQuery, ListNamespacesResponse, LoadTableResponse, NamespaceIdent, Result,
    TableIdent, Transaction, UpdateNamespacePropertiesRequest, UpdateNamespacePropertiesResponse,
    ViewMetadataWithLocation,
};
use std::ops::Deref;

use crate::api::iceberg::v1::Prefix;
use crate::api::ThreadSafe as ServiceState;
pub use crate::api::{ErrorModel, IcebergErrorResponse};
use http::StatusCode;
use std::str::FromStr;

use crate::service::contract_verification::ContractVerifiers;
use crate::service::event_publisher::CloudEventsPublisher;
pub use secrets::{SecretIdent, SecretStore};

use self::auth::AuthZHandler;

#[async_trait::async_trait]
pub trait NamespaceIdentExt
where
    Self: Sized,
{
    fn parent(&self) -> Option<NamespaceIdent>;
}

#[async_trait::async_trait]
impl NamespaceIdentExt for NamespaceIdent {
    fn parent(&self) -> Option<Self> {
        let mut name = self.clone().inner();
        // The last element is the namespace itself, everything before it the parent.
        name.pop();

        if name.is_empty() {
            None
        } else {
            match NamespaceIdent::from_vec(name) {
                Ok(ident) => Some(ident),
                // This only fails if the vector is empty,
                // in which case there is no parent, so return None
                Err(_e) => None,
            }
        }
    }
}

// ---------------- State ----------------

#[derive(Clone, Debug)]
pub struct State<A: AuthZHandler, C: Catalog, S: SecretStore> {
    pub auth: A::State,
    pub catalog: C::State,
    pub secrets: S::State,
    pub publisher: CloudEventsPublisher,
    pub contract_verifiers: ContractVerifiers,
}

impl<A: AuthZHandler, C: Catalog, S: SecretStore> ServiceState for State<A, C, S> {}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Copy)]
pub struct NamespaceIdentUuid(uuid::Uuid);

impl Deref for NamespaceIdentUuid {
    type Target = uuid::Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for NamespaceIdentUuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for NamespaceIdentUuid {
    type Err = IcebergErrorResponse;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(NamespaceIdentUuid(uuid::Uuid::from_str(s).map_err(
            |e| {
                ErrorModel::builder()
                    .code(StatusCode::BAD_REQUEST.into())
                    .message("Provided namespace id is not a valid UUID".to_string())
                    .r#type("NamespaceIDIsNotUUID".to_string())
                    .stack(Some(vec![e.to_string()]))
                    .build()
            },
        )?))
    }
}

impl From<uuid::Uuid> for NamespaceIdentUuid {
    fn from(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Copy)]
pub struct TableIdentUuid(uuid::Uuid);

impl std::fmt::Display for TableIdentUuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for TableIdentUuid {
    type Target = uuid::Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<uuid::Uuid> for TableIdentUuid {
    fn from(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }
}

impl FromStr for TableIdentUuid {
    type Err = IcebergErrorResponse;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(TableIdentUuid(uuid::Uuid::from_str(s).map_err(|e| {
            ErrorModel::builder()
                .code(StatusCode::BAD_REQUEST.into())
                .message("Provided table id is not a valid UUID".to_string())
                .r#type("TableIDIsNotUUID".to_string())
                .stack(Some(vec![e.to_string()]))
                .build()
        })?))
    }
}

// ---------------- Identifier ----------------
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Copy)]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
// Is UUID here too strict?
pub struct ProjectIdent(uuid::Uuid);

impl Deref for ProjectIdent {
    type Target = uuid::Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for ProjectIdent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ProjectIdent {
    type Err = IcebergErrorResponse;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ProjectIdent(uuid::Uuid::from_str(s).map_err(|e| {
            ErrorModel::builder()
                .code(StatusCode::BAD_REQUEST.into())
                .message("Provided project id is not a valid UUID".to_string())
                .r#type("ProjectIDIsNotUUID".to_string())
                .stack(Some(vec![e.to_string()]))
                .build()
        })?))
    }
}

/// Status of a warehouse
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    strum_macros::Display,
    serde::Serialize,
    serde::Deserialize,
    utoipa::ToSchema,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(
    feature = "sqlx",
    sqlx(type_name = "warehouse_status", rename_all = "kebab-case")
)]
pub enum WarehouseStatus {
    /// The warehouse is active and can be used
    Active,
    /// The warehouse is inactive and cannot be used.
    Inactive,
}

impl sqlx::postgres::PgHasArrayType for WarehouseStatus {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("_warehouse_status")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Copy)]
#[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
#[cfg_attr(feature = "sqlx", sqlx(transparent))]
pub struct WarehouseIdent(uuid::Uuid);

impl WarehouseIdent {
    #[must_use]
    pub fn to_uuid(&self) -> uuid::Uuid {
        **self
    }

    #[must_use]
    pub fn as_uuid(&self) -> &uuid::Uuid {
        self
    }
}

impl Deref for WarehouseIdent {
    type Target = uuid::Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<uuid::Uuid> for WarehouseIdent {
    fn from(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }
}

impl std::fmt::Display for WarehouseIdent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for WarehouseIdent {
    type Err = IcebergErrorResponse;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(WarehouseIdent(uuid::Uuid::from_str(s).map_err(|e| {
            ErrorModel::builder()
                .code(StatusCode::BAD_REQUEST.into())
                .message("Provided warehouse id is not a valid UUID".to_string())
                .r#type("WarehouseIDIsNotUUID".to_string())
                .stack(Some(vec![e.to_string()]))
                .build()
        })?))
    }
}

impl From<uuid::Uuid> for ProjectIdent {
    fn from(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }
}

impl TryFrom<Prefix> for WarehouseIdent {
    type Error = IcebergErrorResponse;

    fn try_from(value: Prefix) -> Result<Self, Self::Error> {
        let prefix = uuid::Uuid::parse_str(value.as_str()).map_err(|e| {
            ErrorModel::builder()
                .code(StatusCode::BAD_REQUEST.into())
                .message(format!(
                    "Provided prefix is not a warehouse id. Expected UUID, got: {}",
                    value.as_str()
                ))
                .r#type("PrefixIsNotWarehouseID".to_string())
                .stack(Some(vec![e.to_string()]))
                .build()
        })?;
        Ok(WarehouseIdent(prefix))
    }
}
