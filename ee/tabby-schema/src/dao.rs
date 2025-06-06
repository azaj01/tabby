use anyhow::bail;
use hash_ids::HashIds;
use lazy_static::lazy_static;
use tabby_db::{
    AttachmentClientCode, AttachmentCode, AttachmentCodeFileList, AttachmentCommitDoc,
    AttachmentDoc, AttachmentIngestedDoc, AttachmentIssueDoc, AttachmentPageDoc, AttachmentPullDoc,
    AttachmentWebDoc, EmailSettingDAO, IngestedDocumentDAO, IngestedDocumentStatusDAO,
    IngestionStatusDAO, IntegrationDAO, InvitationDAO, JobRunDAO, LdapCredentialDAO,
    NotificationDAO, OAuthCredentialDAO, PageDAO, ServerSettingDAO, ThreadDAO, UserEventDAO,
};

use crate::{
    auth::LdapEncryptionKind,
    ingestion::IngestionStats,
    integration::{Integration, IntegrationKind, IntegrationStatus},
    interface::UserValue,
    notification::{Notification, NotificationRecipient},
    page,
    repository::RepositoryKind,
    retrieval,
    schema::{
        auth::{self, LdapCredential, OAuthCredential, OAuthProvider},
        email::{AuthMethod, EmailSetting, Encryption},
        ingestion::{IngestedDocStatus, IngestedDocument},
        job,
        repository::{
            GithubRepositoryProvider, GitlabRepositoryProvider, RepositoryProviderStatus,
        },
        setting::{NetworkSetting, SecuritySetting},
        user_event::{EventKind, UserEvent},
        CoreError,
    },
    thread::{self},
};

impl From<InvitationDAO> for auth::Invitation {
    fn from(val: InvitationDAO) -> Self {
        Self {
            id: (val.id as i32).as_id(),
            email: val.email,
            code: val.code,
            created_at: val.created_at,
        }
    }
}

impl From<JobRunDAO> for job::JobRun {
    fn from(run: JobRunDAO) -> Self {
        Self {
            id: run.id.as_id(),
            job: run.name,
            created_at: run.created_at,
            updated_at: run.updated_at,
            started_at: run.started_at,
            finished_at: run.finished_at,
            exit_code: run.exit_code.map(|i| i as i32),
            stdout: run.stdout,
        }
    }
}

impl TryFrom<OAuthCredentialDAO> for OAuthCredential {
    type Error = anyhow::Error;

    fn try_from(val: OAuthCredentialDAO) -> Result<Self, Self::Error> {
        Ok(OAuthCredential {
            provider: OAuthProvider::from_enum_str(&val.provider)?,
            client_id: val.client_id,
            created_at: val.created_at,
            updated_at: val.updated_at,
            client_secret: val.client_secret,
        })
    }
}

impl TryFrom<LdapCredentialDAO> for LdapCredential {
    type Error = anyhow::Error;

    fn try_from(val: LdapCredentialDAO) -> Result<Self, Self::Error> {
        Ok(LdapCredential {
            host: val.host,
            port: val.port as i32,
            bind_dn: val.bind_dn,
            base_dn: val.base_dn,
            user_filter: val.user_filter,
            encryption: LdapEncryptionKind::from_enum_str(&val.encryption)?,
            skip_tls_verify: val.skip_tls_verify,
            email_attribute: val.email_attribute,
            name_attribute: val.name_attribute,
            created_at: val.created_at,
            updated_at: val.updated_at,
        })
    }
}

impl TryFrom<EmailSettingDAO> for EmailSetting {
    type Error = anyhow::Error;

    fn try_from(value: EmailSettingDAO) -> Result<Self, Self::Error> {
        let encryption = Encryption::from_enum_str(&value.encryption)?;
        let auth_method = AuthMethod::from_enum_str(&value.auth_method)?;

        Ok(EmailSetting {
            smtp_username: value.smtp_username,
            smtp_server: value.smtp_server,
            smtp_port: value.smtp_port as i32,
            from_address: value.from_address,
            encryption,
            auth_method,
        })
    }
}

impl From<ServerSettingDAO> for SecuritySetting {
    fn from(value: ServerSettingDAO) -> Self {
        Self {
            allowed_register_domain_list: value
                .security_allowed_register_domain_list()
                .map(|s| s.to_owned())
                .collect(),
            disable_client_side_telemetry: value.security_disable_client_side_telemetry,
            disable_password_login: value.security_disable_password_login,
        }
    }
}

impl From<ServerSettingDAO> for NetworkSetting {
    fn from(value: ServerSettingDAO) -> Self {
        Self {
            external_url: value.network_external_url,
        }
    }
}

impl TryFrom<IntegrationDAO> for Integration {
    type Error = anyhow::Error;
    fn try_from(value: IntegrationDAO) -> anyhow::Result<Self> {
        let status = if value.synced && value.error.is_none() {
            IntegrationStatus::Ready
        } else if value.error.is_some() {
            IntegrationStatus::Failed
        } else {
            IntegrationStatus::Pending
        };
        Ok(Self {
            id: value.id.as_id(),
            kind: IntegrationKind::from_enum_str(&value.kind)?,
            display_name: value.display_name,
            access_token: value.access_token,
            api_base: value.api_base,
            created_at: value.created_at,
            updated_at: value.updated_at,
            status,
            message: value.error,
        })
    }
}

impl From<IntegrationKind> for RepositoryKind {
    fn from(value: IntegrationKind) -> Self {
        match value {
            IntegrationKind::Github => RepositoryKind::Github,
            IntegrationKind::Gitlab => RepositoryKind::Gitlab,
            IntegrationKind::GithubSelfHosted => RepositoryKind::GithubSelfHosted,
            IntegrationKind::GitlabSelfHosted => RepositoryKind::GitlabSelfHosted,
        }
    }
}

impl From<Integration> for GithubRepositoryProvider {
    fn from(value: Integration) -> Self {
        Self {
            id: value.id,
            display_name: value.display_name,
            status: value.status.into(),
            access_token: Some(value.access_token),
            api_base: value.api_base,
        }
    }
}

impl From<Integration> for GitlabRepositoryProvider {
    fn from(value: Integration) -> Self {
        Self {
            id: value.id,
            display_name: value.display_name,
            status: value.status.into(),
            access_token: Some(value.access_token),
            api_base: value.api_base,
        }
    }
}

impl From<IntegrationStatus> for RepositoryProviderStatus {
    fn from(value: IntegrationStatus) -> Self {
        match value {
            IntegrationStatus::Ready => Self::Ready,
            IntegrationStatus::Pending => Self::Pending,
            IntegrationStatus::Failed => Self::Failed,
        }
    }
}

impl TryFrom<UserEventDAO> for UserEvent {
    type Error = anyhow::Error;
    fn try_from(value: UserEventDAO) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id.as_id(),
            user_id: value.user_id.as_id(),
            kind: EventKind::from_enum_str(&value.kind)?,
            created_at: value.created_at,
            payload: String::from_utf8(value.payload)?,
        })
    }
}

impl From<NotificationDAO> for Notification {
    fn from(value: NotificationDAO) -> Self {
        Self {
            id: value.id.as_id(),
            content: value.content,
            read: value.read,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

impl From<AttachmentCode> for thread::MessageAttachmentCode {
    fn from(value: AttachmentCode) -> Self {
        Self {
            git_url: value.git_url,
            commit: value.commit,
            filepath: value.filepath,
            language: value.language,
            content: value.content,
            start_line: value.start_line.map(|x| x as i32),
        }
    }
}

impl From<&thread::MessageAttachmentCode> for AttachmentCode {
    fn from(val: &thread::MessageAttachmentCode) -> Self {
        AttachmentCode {
            git_url: val.git_url.clone(),
            commit: val.commit.clone(),
            filepath: val.filepath.clone(),
            language: val.language.clone(),
            content: val.content.clone(),
            start_line: val.start_line.map(|x| x as usize),
        }
    }
}

impl From<AttachmentClientCode> for thread::MessageAttachmentClientCode {
    fn from(value: AttachmentClientCode) -> Self {
        Self {
            filepath: value.filepath,
            content: value.content,
            start_line: value.start_line.map(|x| x as i32),
        }
    }
}

impl From<&thread::MessageAttachmentCodeInput> for AttachmentClientCode {
    fn from(val: &thread::MessageAttachmentCodeInput) -> Self {
        AttachmentClientCode {
            filepath: val.filepath.clone(),
            content: val.content.clone(),
            start_line: val.start_line.map(|x| x as usize),
        }
    }
}

impl From<AttachmentCodeFileList> for thread::MessageAttachmentCodeFileList {
    fn from(value: AttachmentCodeFileList) -> Self {
        Self {
            file_list: value.file_list,
            truncated: value.truncated,
        }
    }
}

pub fn from_thread_message_attachment_document(
    doc: AttachmentDoc,
    author: Option<UserValue>,
) -> thread::MessageAttachmentDoc {
    match doc {
        AttachmentDoc::Web(web) => {
            thread::MessageAttachmentDoc::Web(thread::MessageAttachmentWebDoc {
                title: web.title,
                link: web.link,
                content: web.content,
            })
        }
        AttachmentDoc::Issue(issue) => {
            thread::MessageAttachmentDoc::Issue(thread::MessageAttachmentIssueDoc {
                title: issue.title,
                link: issue.link,
                author,
                body: issue.body,
                closed: issue.closed,
            })
        }
        AttachmentDoc::Pull(pull) => {
            thread::MessageAttachmentDoc::Pull(thread::MessageAttachmentPullDoc {
                title: pull.title,
                link: pull.link,
                author,
                body: pull.body,
                patch: pull.diff,
                merged: pull.merged,
            })
        }
        AttachmentDoc::Commit(commit) => {
            thread::MessageAttachmentDoc::Commit(thread::MessageAttachmentCommitDoc {
                sha: commit.sha,
                message: commit.message,
                author,
                author_at: commit.author_at,
            })
        }
        AttachmentDoc::Page(page) => {
            thread::MessageAttachmentDoc::Page(thread::MessageAttachmentPageDoc {
                link: page.page_link,
                title: page.title,
                content: page.content,
            })
        }
        AttachmentDoc::Ingested(ingested) => {
            thread::MessageAttachmentDoc::Ingested(thread::MessageAttachmentIngestedDoc {
                id: ingested.id,
                link: ingested.link,
                title: ingested.title,
                body: ingested.body,
            })
        }
    }
}

impl From<&thread::MessageAttachmentDoc> for AttachmentDoc {
    fn from(val: &thread::MessageAttachmentDoc) -> Self {
        match val {
            thread::MessageAttachmentDoc::Web(val) => AttachmentDoc::Web(AttachmentWebDoc {
                title: val.title.clone(),
                link: val.link.clone(),
                content: val.content.clone(),
            }),
            thread::MessageAttachmentDoc::Issue(val) => AttachmentDoc::Issue(AttachmentIssueDoc {
                title: val.title.clone(),
                link: val.link.clone(),
                author_user_id: val.author.as_ref().map(|x| match x {
                    UserValue::UserSecured(user) => user.id.to_string(),
                }),
                body: val.body.clone(),
                closed: val.closed,
            }),
            thread::MessageAttachmentDoc::Pull(val) => AttachmentDoc::Pull(AttachmentPullDoc {
                title: val.title.clone(),
                link: val.link.clone(),
                author_user_id: val.author.as_ref().map(|x| match x {
                    UserValue::UserSecured(user) => user.id.to_string(),
                }),
                body: val.body.clone(),
                diff: val.patch.clone(),
                merged: val.merged,
            }),
            thread::MessageAttachmentDoc::Commit(val) => {
                AttachmentDoc::Commit(AttachmentCommitDoc {
                    sha: val.sha.clone(),
                    message: val.message.clone(),
                    author_user_id: val.author.as_ref().map(|x| match x {
                        UserValue::UserSecured(user) => user.id.to_string(),
                    }),
                    author_at: val.author_at,
                })
            }
            thread::MessageAttachmentDoc::Page(val) => AttachmentDoc::Page(AttachmentPageDoc {
                page_link: val.link.clone(),
                title: val.title.clone(),
                content: val.content.clone(),
            }),
            thread::MessageAttachmentDoc::Ingested(val) => {
                AttachmentDoc::Ingested(AttachmentIngestedDoc {
                    id: val.id.clone(),
                    link: val.link.clone(),
                    title: val.title.clone(),
                    body: val.body.clone(),
                })
            }
        }
    }
}

impl From<ThreadDAO> for thread::Thread {
    fn from(value: ThreadDAO) -> Self {
        Self {
            id: value.id.as_id(),
            user_id: value.user_id.as_id(),
            is_ephemeral: value.is_ephemeral,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

impl From<PageDAO> for page::Page {
    fn from(value: PageDAO) -> Self {
        Self {
            id: value.id.as_id(),
            author_id: value.author_id.as_id(),
            title: value.title,
            code_source_id: value.code_source_id,
            content: value.content,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

impl From<&retrieval::AttachmentCode> for AttachmentCode {
    fn from(value: &retrieval::AttachmentCode) -> Self {
        Self {
            git_url: value.git_url.clone(),
            commit: value.commit.clone(),
            filepath: value.filepath.clone(),
            language: value.language.clone(),
            content: value.content.clone(),
            start_line: value.start_line.map(|x| x as usize),
        }
    }
}

impl From<&AttachmentCode> for retrieval::AttachmentCode {
    fn from(value: &AttachmentCode) -> Self {
        Self {
            git_url: value.git_url.clone(),
            commit: value.commit.clone(),
            filepath: value.filepath.clone(),
            language: value.language.clone(),
            content: value.content.clone(),
            start_line: value.start_line.map(|x| x as i32),
        }
    }
}

impl From<AttachmentCodeFileList> for retrieval::AttachmentCodeFileList {
    fn from(value: AttachmentCodeFileList) -> Self {
        Self {
            file_list: value.file_list,
            truncated: value.truncated,
        }
    }
}

impl From<&retrieval::AttachmentDoc> for AttachmentDoc {
    fn from(value: &retrieval::AttachmentDoc) -> Self {
        match value {
            retrieval::AttachmentDoc::Web(web) => AttachmentDoc::Web(AttachmentWebDoc {
                title: web.title.clone(),
                link: web.link.clone(),
                content: web.content.clone(),
            }),
            retrieval::AttachmentDoc::Issue(issue) => AttachmentDoc::Issue(AttachmentIssueDoc {
                title: issue.title.clone(),
                link: issue.link.clone(),
                author_user_id: issue.author.as_ref().map(|x| match x {
                    UserValue::UserSecured(user) => user.id.to_string(),
                }),
                body: issue.body.clone(),
                closed: issue.closed,
            }),
            retrieval::AttachmentDoc::Pull(pull) => AttachmentDoc::Pull(AttachmentPullDoc {
                title: pull.title.clone(),
                link: pull.link.clone(),
                author_user_id: pull.author.as_ref().map(|x| match x {
                    UserValue::UserSecured(user) => user.id.to_string(),
                }),
                body: pull.body.clone(),
                diff: pull.diff.clone(),
                merged: pull.merged,
            }),
            retrieval::AttachmentDoc::Commit(commit) => {
                AttachmentDoc::Commit(AttachmentCommitDoc {
                    sha: commit.sha.clone(),
                    message: commit.message.clone(),
                    author_user_id: commit.author.as_ref().map(|x| match x {
                        UserValue::UserSecured(user) => user.id.to_string(),
                    }),
                    author_at: commit.author_at,
                })
            }
            retrieval::AttachmentDoc::Page(page) => AttachmentDoc::Page(AttachmentPageDoc {
                page_link: page.link.clone(),
                title: page.title.clone(),
                content: page.content.clone(),
            }),
            retrieval::AttachmentDoc::Ingested(ingested) => {
                AttachmentDoc::Ingested(AttachmentIngestedDoc {
                    id: ingested.id.clone(),
                    link: ingested.link.clone(),
                    title: ingested.title.clone(),
                    body: ingested.body.clone(),
                })
            }
        }
    }
}

impl From<IngestedDocumentStatusDAO> for IngestedDocStatus {
    fn from(value: IngestedDocumentStatusDAO) -> Self {
        match value {
            IngestedDocumentStatusDAO::Pending => IngestedDocStatus::Pending,
            IngestedDocumentStatusDAO::Failed => IngestedDocStatus::Failed,
            IngestedDocumentStatusDAO::Indexed => IngestedDocStatus::Indexed,
        }
    }
}

impl From<IngestedDocumentDAO> for IngestedDocument {
    fn from(value: IngestedDocumentDAO) -> Self {
        Self {
            id: value.doc_id,
            source: value.source,
            link: value.link,
            title: value.title,
            body: value.body,
            status: value.status.into(),
        }
    }
}

impl From<IngestionStatusDAO> for IngestionStats {
    fn from(value: IngestionStatusDAO) -> Self {
        Self {
            source: value.source,
            pending: value.pending,
            failed: value.failed,
            total: value.total,
        }
    }
}

lazy_static! {
    static ref HASHER: HashIds = HashIds::builder()
        .with_salt("tabby-id-serializer")
        .with_min_length(6)
        .finish();
}

pub trait AsRowid {
    fn as_rowid(&self) -> std::result::Result<i64, CoreError>;
}

impl AsRowid for juniper::ID {
    fn as_rowid(&self) -> std::result::Result<i64, CoreError> {
        HASHER
            .decode(self)
            .and_then(|x| x.first().map(|i| *i as i64))
            .ok_or(CoreError::InvalidID)
    }
}

pub trait AsID {
    fn as_id(&self) -> juniper::ID;
}

impl AsID for i64 {
    fn as_id(&self) -> juniper::ID {
        juniper::ID::new(HASHER.encode(&[*self as u64]))
    }
}

impl AsID for i32 {
    fn as_id(&self) -> juniper::ID {
        (*self as i64).as_id()
    }
}

pub trait DbEnum: Sized {
    fn as_enum_str(&self) -> &'static str;
    fn from_enum_str(s: &str) -> anyhow::Result<Self>;
}

impl DbEnum for EventKind {
    fn as_enum_str(&self) -> &'static str {
        match self {
            EventKind::Completion => "completion",
            EventKind::ChatCompletion => "chat_completion",
            EventKind::Select => "select",
            EventKind::View => "view",
            EventKind::Dismiss => "dismiss",
        }
    }

    fn from_enum_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "completion" => Ok(EventKind::Completion),
            "chat_completion" => Ok(EventKind::ChatCompletion),
            "select" => Ok(EventKind::Select),
            "view" => Ok(EventKind::View),
            "dismiss" => Ok(EventKind::Dismiss),
            _ => bail!("{s} is not a valid value for EventKind"),
        }
    }
}

impl DbEnum for IntegrationKind {
    fn as_enum_str(&self) -> &'static str {
        match self {
            IntegrationKind::Github => "github",
            IntegrationKind::Gitlab => "gitlab",
            IntegrationKind::GithubSelfHosted => "github_self_hosted",
            IntegrationKind::GitlabSelfHosted => "gitlab_self_hosted",
        }
    }

    fn from_enum_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "github" => Ok(IntegrationKind::Github),
            "gitlab" => Ok(IntegrationKind::Gitlab),
            "github_self_hosted" => Ok(IntegrationKind::GithubSelfHosted),
            "gitlab_self_hosted" => Ok(IntegrationKind::GitlabSelfHosted),
            _ => bail!("{s} is not a valid value for ProviderKind"),
        }
    }
}

impl DbEnum for Encryption {
    fn as_enum_str(&self) -> &'static str {
        match self {
            Encryption::StartTls => "starttls",
            Encryption::SslTls => "ssltls",
            Encryption::None => "none",
        }
    }

    fn from_enum_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "starttls" => Ok(Encryption::StartTls),
            "ssltls" => Ok(Encryption::SslTls),
            "none" => Ok(Encryption::None),
            _ => bail!("{s} is not a valid value for Encryption"),
        }
    }
}

impl DbEnum for OAuthProvider {
    fn as_enum_str(&self) -> &'static str {
        match self {
            OAuthProvider::Google => "google",
            OAuthProvider::Github => "github",
            OAuthProvider::Gitlab => "gitlab",
        }
    }

    fn from_enum_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "github" => Ok(OAuthProvider::Github),
            "google" => Ok(OAuthProvider::Google),
            "gitlab" => Ok(OAuthProvider::Gitlab),
            _ => bail!("Invalid OAuth credential type"),
        }
    }
}

impl DbEnum for LdapEncryptionKind {
    fn as_enum_str(&self) -> &'static str {
        match self {
            LdapEncryptionKind::None => "none",
            LdapEncryptionKind::StartTLS => "starttls",
            LdapEncryptionKind::LDAPS => "ldaps",
        }
    }

    fn from_enum_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "none" => Ok(LdapEncryptionKind::None),
            "starttls" => Ok(LdapEncryptionKind::StartTLS),
            "ldaps" => Ok(LdapEncryptionKind::LDAPS),
            _ => bail!("Invalid Ldap encryption kind"),
        }
    }
}

impl DbEnum for AuthMethod {
    fn as_enum_str(&self) -> &'static str {
        match self {
            AuthMethod::None => "none",
            AuthMethod::Plain => "plain",
            AuthMethod::Login => "login",
        }
    }

    fn from_enum_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "none" => Ok(AuthMethod::None),
            "plain" => Ok(AuthMethod::Plain),
            "login" => Ok(AuthMethod::Login),
            _ => bail!("{s} is not a valid value for AuthMethod"),
        }
    }
}

impl DbEnum for thread::Role {
    fn as_enum_str(&self) -> &'static str {
        match self {
            thread::Role::Assistant => "assistant",
            thread::Role::User => "user",
        }
    }

    fn from_enum_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "assistant" => Ok(thread::Role::Assistant),
            "user" => Ok(thread::Role::User),
            _ => bail!("{s} is not a valid value for thread::Role"),
        }
    }
}

impl DbEnum for NotificationRecipient {
    fn as_enum_str(&self) -> &'static str {
        match self {
            NotificationRecipient::Admin => "admin",
            NotificationRecipient::AllUser => "all_user",
        }
    }

    fn from_enum_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "admin" => Ok(NotificationRecipient::Admin),
            "all_user" => Ok(NotificationRecipient::AllUser),
            _ => bail!("{s} is not a valid value for NotificationKind"),
        }
    }
}
