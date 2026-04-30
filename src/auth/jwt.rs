//! # JSON Web Token (JWT) and Password Hashing
//!
//! This module provides functionality for working with JSON Web Tokens (JWTs)
//! and password hashing.
use jsonwebtoken::{
    decode, encode, errors::Result as JWTResult, get_current_timestamp, Algorithm, DecodingKey,
    EncodingKey, Header, TokenData, Validation,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Represents the default JWT algorithm used by the [`JWT`] struct.
const JWT_ALGORITHM: Algorithm = Algorithm::HS512;

/// Represents the claims associated with a user JWT.
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Serialize, Deserialize)]
pub struct UserClaims {
    pub pid: String,
    exp: u64,
    #[serde(default, flatten)]
    pub claims: Map<String, Value>,
}

/// Represents the JWT configuration and operations.
///
/// # Example
/// ```rust
/// use loco_rs::auth;
///
/// auth::jwt::JWT::new("PqRwLF2rhHe8J22oBeHy");
/// ```
#[derive(Debug)]
pub struct JWT {
    secret: String,
    algorithm: Algorithm,
}

impl JWT {
    /// Creates a new [`JWT`] instance with the specified secret key.
    #[must_use]
    pub fn new(secret: &str) -> Self {
        Self {
            secret: secret.to_string(),
            algorithm: JWT_ALGORITHM,
        }
    }

    /// Override the default  JWT algorithm to be used.
    #[must_use]
    pub fn algorithm(mut self, algorithm: Algorithm) -> Self {
        self.algorithm = algorithm;
        self
    }

    /// Generates a new JWT with specified claims and an expiration time.
    ///
    /// # Errors
    ///
    /// returns [`JWTResult`] error when could not generate JWT token. can be an
    /// invalid secret.
    ///
    /// # Example
    /// ```rust
    /// use serde_json::Map;
    /// use loco_rs::auth;
    ///
    /// auth::jwt::JWT::new("PqRwLF2rhHe8J22oBeHy").generate_token(604800, "PID".to_string(), Map::new());
    /// ```
    pub fn generate_token(
        &self,
        expiration: u64,
        pid: String,
        claims: Map<String, Value>,
    ) -> JWTResult<String> {
        let exp = get_current_timestamp().saturating_add(expiration);

        let claims = UserClaims { pid, exp, claims };

        let token = encode(
            &Header::new(self.algorithm),
            &claims,
            &EncodingKey::from_base64_secret(&self.secret)?,
        )?;

        Ok(token)
    }

    /// Validates the authenticity and expiration of a given JWT.
    /// If Token is valid, decode the Token Claims.
    ///
    /// # Errors
    ///
    /// returns [`JWTResult`] error when could not convert the given token to
    /// [`UserClaims`], if the `secret` is invalid or token is expired.
    ///
    /// # Example
    /// ```rust
    /// use loco_rs::auth;
    /// auth::jwt::JWT::new("PqRwLF2rhHe8J22oBeHy").validate("JWT-TOKEN");
    /// ```
    pub fn validate(&self, token: &str) -> JWTResult<TokenData<UserClaims>> {
        let mut validate = Validation::new(self.algorithm);
        validate.leeway = 0;

        decode::<UserClaims>(
            token,
            &DecodingKey::from_base64_secret(&self.secret)?,
            &validate,
        )
    }
}

