//! This module defines an [`EmailSender`] responsible for sending emails using
//! either the SMTP protocol. It includes an asynchronous method `mail` for
//! sending emails with options like sender, recipient, subject, and content.

use lettre::{
    message::{header, MultiPart},
    transport::smtp::{authentication::Credentials, extension::ClientId},
    AsyncTransport, Message, Tokio1Executor, Transport,
};
use tracing::error;

use super::{Email, Result, DEFAULT_FROM_SENDER};
use crate::{config, errors::Error};

/// An enumeration representing the possible transport methods for sending
/// emails.
#[derive(Clone, Debug)]
pub enum EmailTransport {
    /// SMTP (Simple Mail Transfer Protocol) transport.
    Smtp(lettre::AsyncSmtpTransport<lettre::Tokio1Executor>),
    /// Test/stub transport for testing purposes.
    Test(lettre::transport::stub::StubTransport),
}

/// A structure representing the email sender, encapsulating the chosen
/// transport method.
#[derive(Clone, Debug)]
pub struct EmailSender {
    pub transport: EmailTransport,
}

impl EmailSender {
    /// Creates a new `EmailSender` using the SMTP transport method based on the
    /// provided SMTP configuration.
    ///
    /// # Errors
    ///
    /// when could not initialize SMTP transport
    pub fn smtp(config: &config::SmtpMailer) -> Result<Self> {
        let mut email_builder = if config.secure {
            lettre::AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
                .map_err(|error| {
                    error!(err.msg = %error, err.detail = ?error, "smtp_init_error");
                    error
                })?
                .port(config.port)
        } else {
            lettre::AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
                .port(config.port)
        };

        if let Some(auth) = config.auth.as_ref() {
            email_builder = email_builder
                .credentials(Credentials::new(auth.user.clone(), auth.password.clone()));
        }

        if let Some(hello_name) = config.hello_name.as_ref() {
            email_builder = email_builder.hello_name(ClientId::Domain(hello_name.clone()));
        }

        Ok(Self {
            transport: EmailTransport::Smtp(email_builder.build()),
        })
    }

    #[must_use]
    pub fn stub() -> Self {
        Self {
            transport: EmailTransport::Test(lettre::transport::stub::StubTransport::new_ok()),
        }
    }

    /// Sends an email using the configured transport method.
    ///
    /// # Errors
    ///
    /// When email doesn't send successfully or has an error to build the
    /// message
    pub async fn mail(&self, email: &Email) -> Result<()> {
        let content = MultiPart::alternative_plain_html(email.text.clone(), email.html.clone());
        let mut builder = Message::builder()
            .from(
                email
                    .from
                    .clone()
                    .unwrap_or_else(|| DEFAULT_FROM_SENDER.to_string())
                    .parse()?,
            )
            .to(email.to.parse()?);

        if let Some(bcc) = &email.bcc {
            builder = builder.bcc(bcc.parse()?);
        }

        if let Some(cc) = &email.cc {
            builder = builder.cc(cc.parse()?);
        }

        if let Some(reply_to) = &email.reply_to {
            builder = builder.reply_to(reply_to.parse()?);
        }

        if let Some(headers) = &email.headers {
            if let Some(references) = &headers.references {
                builder = builder.header(header::References::from(references.clone()));
            }
            if let Some(in_reply_to) = &headers.in_reply_to {
                builder = builder.header(header::InReplyTo::from(in_reply_to.clone()));
            }
            if let Some(message_id) = &headers.message_id {
                builder = builder.header(header::MessageId::from(message_id.clone()));
            }
        }

        let msg = builder
            .subject(email.subject.clone())
            .multipart(content)
            .map_err(|error| {
                error!(err.msg = %error, err.detail = ?error, "email_building_error");
                error
            })?;

        match &self.transport {
            EmailTransport::Smtp(xp) => {
                xp.send(msg).await?;
            }
            EmailTransport::Test(xp) => {
                xp.send(&msg)
                    .map_err(|e| Error::Message(format!("sending email error: {e}")))?;
            }
        }
        Ok(())
    }
}

