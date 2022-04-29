use actix_web::{dev::Payload, Error, FromRequest, HttpRequest};
use futures::future::{err, ok, Ready};

use jsonwebtoken::{
    decode, encode, errors as JError, Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::convert::TryFrom;
use xerror::api::JWTError;

use time::get_time;

use xerror::api::*;

lazy_static::lazy_static! {
    /// This is the secret key with which we sign the JWT tokens.
    static ref KEY: Box<[u8]> = match std::env::var_os("SECRET_KEY")
        .and_then(|x| x.to_str().map(ToOwned::to_owned))
    {
        Some(x) => x.into_boxed_str().into_boxed_bytes(),
        None => {
            eprintln!("The env `SECRET_KEY` is either not set, or not valid ascii");
            b"a".repeat(32).into_boxed_slice()
        },
    };
    static ref E_KEY: EncodingKey = EncodingKey::from_secret(&KEY);
    static ref D_KEY: DecodingKey<'static> = DecodingKey::from_secret(&KEY);
}

/// Enum represents a role set for a given key.
#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub enum UserRoles {
    /// MasterToken is the main auth token that can modify user perms
    MasterToken,
    /// Api tokens are tokens that can only r/w to api endpoints
    ApiToken(HashSet<ApiRole>),
}

/// Enum represents the available api roles that can be applied to api tokens.
#[derive(Debug, Eq, PartialEq, Clone, Copy, Serialize, Deserialize, Hash)]
pub enum ApiRole {
    /// Represents the role which allows the key to read endpoints.
    ViewOnly,
    /// Represents the role which allows the key to write to endpoints.
    Trade,
    /// Represents the role which allows the key to write to endpoints.
    Transfer,
}

impl std::fmt::Display for ApiRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let output = match self {
            Self::ViewOnly => "ViewOnly",
            Self::Trade => "Trade",
            Self::Transfer => "Transfer",
        };

        write!(f, "{}", output)
    }
}

impl TryFrom<String> for ApiRole {
    type Error = String;
    fn try_from(n: String) -> Result<Self, Self::Error> {
        match n.as_str() {
            "ViewOnly" => Ok(Self::ViewOnly),
            "Trade" => Ok(Self::Trade),
            "Transfer" => Ok(Self::Transfer),
            _ => Err("Couldn't convert role.".into()),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Copy, Serialize, Deserialize, Hash)]
pub enum AuthType {
    Hmac,
    Jwt,
}

/// Struct holds info needed for JWT to function correctly
#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct UserRolesToken {
    /// Timestamp when the token was issued.
    iat: i64,
    /// Timestamp when the token expires.
    exp: i64,
    /// User id
    uid: i32,
    /// Token id
    tid: Option<i32>,
    /// The roles of the user, usually owner or user
    roles: UserRoles,
}

impl UserRolesToken {
    #[inline]
    pub const fn when_expires(&self) -> i64 {
        self.exp
    }

    /// Method returns whether the token is expired or not.
    #[inline]
    pub fn is_expired(&self) -> bool {
        let now = get_time().sec;
        now >= self.exp
    }

    /// Method used to make sure that tokens are generated for different users to avoid collisions
    #[inline]
    pub const fn is_claimed_user(&self, claimed_user: i32) -> bool {
        self.uid == claimed_user
    }

    /// Method checks if the user holding this token has a specific role.
    #[inline]
    pub fn is_master(&self) -> bool {
        self.roles == UserRoles::MasterToken
    }

    /// Method checks if token has some `ApiRole`, if its a master token it always returns true.
    #[inline]
    pub fn has_role(&self, other: ApiRole) -> bool {
        if let UserRoles::ApiToken(roles) = &self.roles {
            roles.contains(&other)
        } else {
            true
        }
    }

    /// Method checks if token has some `ApiRole`, if its a master token it always returns true.
    #[inline]
    pub fn get_roles(&self) -> UserRoles {
        self.roles.clone()
    }

    /// Method checks if token has all of the `ApiRoles` passed in. If the token is a master token
    /// then this always returns true.
    #[inline]
    pub fn has_roles(&self, others: &HashSet<ApiRole>) -> bool {
        if let UserRoles::ApiToken(roles) = &self.roles {
            roles.is_superset(others)
        } else {
            true
        }
    }

    /// Method returns the username from the token
    #[inline]
    pub const fn get_user(&self) -> i32 {
        self.uid
    }

    /// Method returns the token id
    #[inline]
    pub const fn get_tid(&self) -> Option<i32> {
        self.tid
    }

    /// Method converts all roles to a vec of strings
    #[inline]
    pub fn roles_to_string(&self) -> Vec<String> {
        if let UserRoles::ApiToken(roles) = &self.roles {
            roles.iter().cloned().map(|x| x.to_string()).collect::<Vec<String>>()
        } else {
            vec!["Master".into()]
        }
    }
}

impl RefreshToken {
    #[inline]
    pub const fn when_expires(&self) -> i64 {
        self.exp
    }

    /// Method returns whether the token is expired or not.
    #[inline]
    pub fn is_expired(&self) -> bool {
        let now = get_time().sec;
        now >= self.exp
    }

    /// Method used to make sure that tokens are generated for different users to avoid collisions
    #[inline]
    pub const fn is_claimed_user(&self, claimed_user: i32) -> bool {
        self.uid == claimed_user
    }

    /// Method checks if the user holding this token has a specific role.
    #[inline]
    pub fn is_master(&self) -> bool {
        self.roles == UserRoles::MasterToken
    }

    /// Method checks if token has some `ApiRole`, if its a master token it always returns true.
    #[inline]
    pub fn has_role(&self, other: ApiRole) -> bool {
        if let UserRoles::ApiToken(roles) = &self.roles {
            roles.contains(&other)
        } else {
            true
        }
    }

    /// Method checks if token has all of the `ApiRoles` passed in. If the token is a master token
    /// then this always returns true.
    #[inline]
    pub fn has_roles(&self, others: &HashSet<ApiRole>) -> bool {
        if let UserRoles::ApiToken(roles) = &self.roles {
            roles.is_superset(others)
        } else {
            true
        }
    }

    /// Method returns the username from the token
    #[inline]
    pub const fn get_user(&self) -> i32 {
        self.uid
    }

    /// Method converts all roles to a vec of strings
    #[inline]
    pub fn roles_to_string(&self) -> Vec<String> {
        if let UserRoles::ApiToken(roles) = &self.roles {
            roles.iter().cloned().map(|x| x.to_string()).collect::<Vec<String>>()
        } else {
            vec!["Master".into()]
        }
    }
}

/// Struct holds info needed for JWT to function correctly
#[derive(Debug, Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct RefreshToken {
    /// Timestamp when the token was issued.
    iat: i64,
    /// Timestamp when the token expires.
    exp: i64,
    /// User id
    uid: i32,
    /// The roles of the user, usually owner or user
    roles: UserRoles,
}

/// Function generates a new JWT token and signs it with our KEY
/// # Arguments
/// * `user` - Username for whom we want to generate a token
/// * `roles` - vector of roles we want to give to this user.
/// * `tid` - Token id, if this token is to be a API key then you want to supply a token id.
/// otherwise supply `None`.
#[inline]
pub fn jwt_generate(uid: i32, tid: Option<i32>, roles: UserRoles, livetime: i64) -> String {
    let now = get_time().sec;
    let payload = UserRolesToken {
        iat: now,
        exp: now + livetime,
        uid,
        tid,
        roles,
    };

    encode(&Header::new(Algorithm::HS512), &payload, &E_KEY).unwrap()
}

/// Function generates a new JWT token and signs it with our KEY
/// # Arguments
/// * `user` - Username for whom we want to generate a token
/// * `roles` - vector of roles we want to give to this user.
/// * `tid` - Token id, if this token is to be a API key then you want to supply a token id.
/// otherwise supply `None`.
#[inline]
pub fn jwt_generate_refresh_token(uid: i32, roles: UserRoles, lifetime: i64) -> String {
    let now = get_time().sec;
    let payload = RefreshToken {
        iat: now,
        exp: now + lifetime,
        uid,
        roles,
    };

    encode(&Header::new(Algorithm::HS512), &payload, &E_KEY).unwrap()
}

/// Function checks the token supplied and validates it
/// # Arguments
/// * `token` - JWT token we want to validate
#[inline]
pub fn jwt_check(token: &str) -> Result<TokenData<UserRolesToken>, ApiError> {
    decode::<UserRolesToken>(token, &D_KEY, &Validation::new(Algorithm::HS512)).map_err(|e| match e.into_kind() {
        JError::ErrorKind::ExpiredSignature => ApiError::JWT(JWTError::Expired),
        _ => ApiError::JWT(JWTError::Invalid),
    })
}

/// Function checks the renew token supplied and validates it
/// # Arguments
/// * `rewew_token` - JWT token we want to validate
#[inline]
pub fn jwt_check_refresh_token(rewew_token: &str) -> Result<TokenData<RefreshToken>, ApiError> {
    decode::<RefreshToken>(rewew_token, &D_KEY, &Validation::new(Algorithm::HS512)).map_err(|e| match e.into_kind() {
        JError::ErrorKind::ExpiredSignature => ApiError::JWT(JWTError::Expired),
        _ => ApiError::JWT(JWTError::Invalid),
    })
}

/// This struct unifies auth data across
/// multiple authentication methods.
#[derive(Debug, Clone)]
pub struct AuthData {
    pub api_key: Option<String>,
    pub uid: i32,
    pub expiry: Option<i64>,
    pub passphrase: Option<String>,
    pub signature: Option<String>,
    pub timestamp: String,
    pub user_roles: Option<UserRoles>,
    pub auth_type: AuthType,
    pub tid: Option<i32>,
}

impl AuthData {
    #[inline]
    pub fn is_master(&self) -> bool {
        if let Some(roles) = &self.user_roles {
            *roles == UserRoles::MasterToken
        } else {
            false
        }
    }

    /// Method converts all roles to a vec of strings
    #[inline]
    pub fn roles_to_string(&self) -> Vec<String> {
        if let Some(roles) = &self.user_roles {
            if let UserRoles::ApiToken(roles) = &roles {
                roles.iter().cloned().map(|x| x.to_string()).collect::<Vec<String>>()
            } else {
                vec!["Master".into()]
            }
        } else {
            vec![]
        }
    }

    #[inline]
    pub fn has_role(&self, other: ApiRole) -> bool {
        if let Some(user_roles) = &self.user_roles {
            if let UserRoles::ApiToken(roles) = user_roles {
                // TODO trading implies view only. We need a better solution for this.
                if roles.contains(&ApiRole::Trade) && other == ApiRole::ViewOnly {
                    true
                } else {
                    roles.contains(&other)
                }
            } else {
                true
            }
        } else {
            false
        }
    }
}

impl FromRequest for AuthData {
    type Error = Error;
    type Future = Ready<Result<Self, Error>>;

    fn from_request(request: &HttpRequest, _: &mut Payload) -> Self::Future {
        let headers = request.headers();
        if let Some(jwt) = headers.get("authorization") {
            if let Ok(k) = jwt.to_str() {
                match jwt_check(k) {
                    Ok(x) => ok(Self {
                        uid: x.claims.get_user(),
                        auth_type: AuthType::Jwt,
                        expiry: Some(x.claims.when_expires()),
                        user_roles: Some(x.claims.get_roles()),
                        tid: x.claims.get_tid(),
                        timestamp: "0".to_string(),
                        api_key: None,
                        passphrase: None,
                        signature: None,
                    }),
                    Err(e) => err(Error::from(e)),
                }
            } else {
                err(Error::from(ApiError::JWT(JWTError::Invalid)))
            }
        } else {
            err(Error::from(ApiError::JWT(JWTError::NotSupplied)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;
    use std::collections::HashSet;

    macro_rules! set {
    ( $( $x:expr ),* ) => {  // Match zero or more comma delimited items
        {
            let mut temp_set = HashSet::new();  // Create a mutable HashSet
            $(
                temp_set.insert($x); // Insert each item matched into the HashSet
            )*
            temp_set // Return the populated HashSet
        }
    };
}

    #[test]
    async fn test_jwt_generate_check() {
        std::env::set_var("SECRET_KEY", "MYSECRET");
        let token = jwt_generate(123, None, UserRoles::MasterToken, 60 * 60 * 3);
        assert!(jwt_check(&token).is_ok());
    }

    #[test]
    async fn test_jwt_check_invalid() {
        std::env::set_var("SECRET_KEY", "MYSECRET");
        assert!(jwt_check("GO AWAY").is_err());
    }

    #[test]
    async fn test_token_data() {
        std::env::set_var("SECRET_KEY", "MYSECRET");
        let token = jwt_generate(123, None, UserRoles::MasterToken, 60 * 60 * 3);
        let data = jwt_check(&token).unwrap().claims;

        assert!(data.when_expires() > get_time().sec);
        assert!(!data.is_expired());
        assert!(data.is_claimed_user(123));
        assert!(data.is_master());
        assert!(data.has_role(ApiRole::Transfer));
        assert!(data.has_roles(&set![ApiRole::Transfer, ApiRole::Trade]));
        assert!(data.get_tid().is_none());
        assert_eq!(data.get_user(), 123);
        assert_eq!(data.roles_to_string(), vec!["Master".to_string()]);
    }

    #[test]
    async fn test_api_token_data() {
        std::env::set_var("SECRET_KEY", "MYSECRET");
        let token = jwt_generate(123, Some(12), UserRoles::ApiToken(set![ApiRole::ViewOnly]), 60 * 60 * 3);
        let data = jwt_check(&token).unwrap().claims;

        assert!(!data.is_master());
        assert!(data.has_role(ApiRole::ViewOnly));
        assert!(!data.has_role(ApiRole::Trade));
        assert!(data.has_roles(&set![ApiRole::ViewOnly]));
        assert!(!data.has_roles(&set![ApiRole::ViewOnly, ApiRole::Trade]));
        assert_eq!(data.get_tid(), Some(12));
        assert_eq!(data.get_user(), 123);
        assert_eq!(data.roles_to_string(), vec!["ViewOnly".to_string()]);
    }

    #[cfg(feature = "actix")]
    #[ignore]
    #[actix_rt::test]
    async fn test_auth_jwt_missing() {
        let (req, mut payload) = test::TestRequest::with_header("content-type", "text/plain").to_http_parts();
        let resp = <AuthData as FromRequest>::from_request(&req, &mut payload).await;
        assert_eq!(resp.unwrap_err().as_error::<JWTError>(), Some(&JWTError::Missing));
    }

    #[cfg(feature = "actix")]
    #[ignore]
    #[actix_rt::test]
    async fn test_auth_jwt_expired() {
        std::env::set_var("SECRET_KEY", "asd123");

        let now = get_time().sec;
        let payload = UserRolesToken {
            iat: now - 10000,
            exp: now - 1000,
            uid: 123,
            tid: None,
            roles: UserRoles::MasterToken,
        };

        let token = encode(&Header::new(Algorithm::HS512), &payload, &E_KEY).unwrap();

        let (req, mut payload) = test::TestRequest::with_header("authorization", token).to_http_parts();
        let resp = <AuthData as FromRequest>::from_request(&req, &mut payload).await;
        assert_eq!(resp.unwrap_err().as_error::<JWTError>(), Some(&JWTError::Expired));
    }

    #[cfg(feature = "actix")]
    #[ignore]
    #[actix_rt::test]
    async fn test_auth_jwt_invalid() {
        std::env::set_var("SECRET_KEY", "asd123");

        let (req, mut payload) = test::TestRequest::with_header("authorization", "1234").to_http_parts();

        let resp = <AuthData as FromRequest>::from_request(&req, &mut payload).await;
        assert_eq!(resp.unwrap_err().as_error::<JWTError>(), Some(&JWTError::InvalidKey));
    }
}
