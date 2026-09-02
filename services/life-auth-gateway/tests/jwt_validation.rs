use axum::{routing::get, Json, Router};
use chrono::Utc;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use life_auth_gateway::auth::OidcVerifier;
use serde::Serialize;
use serde_json::json;

const RSA_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC59n6mcpZnaZZk
fTKaDXFsoBtQS8YPg9jSBI003QhZ7ktTALCpVQ70vcNADM51S8f+Hf1Ob8gNLufC
Cmw2bIzIsfuZOruaaEBu1JxxqU2jQJ97SQVYNeZOSRmgCEFxIHz5dyTNw78u+eY5
SnmYn1fK1AIkfibJMikKdET9jmh8Y2tf+9uM0s+9u2LDooh2Of8tyLl/LftuHXzr
uLYUfVFC1boZFmk96Elmwnrpm6NzP+gLhpGkWn7VYJ6HXuDlrPRoma9em3z5XMVk
HFwm/PNEj8/m0ljt1/MJJ38zxOXfkNuyGvu4aBl7W7bicrgE8A4gLWdGr98cyXvp
UBcincFXAgMBAAECggEADNlKBUB2lueV6L85Gf7NdEwYXF3CmLuAkCZDXBXc6AXN
y4ref0dYFLNF2deTWPRm+ZkN3E0J6ACdKmw474ZkMHu9CWh5ojYLeaeUCvW/3lnu
iwClGg6t/oEQ1bIBqjTXSGSh34ZjtSBhdq+Szjz4PmnpSfyIu9m3jzgwX3U8T+0a
b6wOj8qKcOk0N/J6lOg2BsPnQW/p9m/xpS8eIGgHCIkGleUdg5nL4g5iVq6OXm3X
knmwKEVII84lURrd79V+jTeeB6VxIRWAArxN/DgulGJkXRAlWypJ5T3S6rDLvZ9u
7kvytovVw0nPYKQG9A5NqYIr28HmQqx40UJDFmT+oQKBgQDkhalcUs2hrQ3mKfCm
ARUnrWz4J4hwCfBP/3YBGRzfW35NraKmLglF7ooP1QMJQMMbKZLDlVP3OOZCVR+S
2h35nVG3XVswd+jnnnGARleELgqOJN98vZTqXKfyP97cvsPgMV2G9/h71FiKHHwX
nUZj5Uts0gkMqiEfhh35ZvXZoQKBgQDQUsepee/L7E7bzmSn+BmkZ42NT5ca/6mY
UDftzFvxw3UPVHMu0wpu8QyD7uvk9zLX0EoPeNf6a2YashmrRPuvI2QNffzkvlyd
mdqSRuib4PlbOqtUO4N4kHkFcmcLBGkI75Ch9hDwnE3noj0XmEvvuOFkam64Luf5
A55q7Dln9wKBgCM3xiYITNCBzwaNqBytRglbXNPRo+FAZtytTg5VRHHXs9tcyxg5
OAyi+nv+I/2lEWx6N7gUp2AOUM4gOEF1g/EYIaPUq10I3cf0TyGptYsVXWMSo66h
uPV1WhynYz052Q4QDY3jYVQUIaEHSsiI4HQ8vicDJ4ngHkKxdKUfDPyBAoGADNhd
2VRcdd1/S0xhpn3Ezv9XmhQDRDXpdivUFwSX0sNzj1tsssFujkKsu+Hah8a6StZc
CrIv1xASPqkmrgnV3wm2nKJdGpmmSk13TbezlhD8LyTh9ZKp26BE5hIUyngeJd/n
siTjDIMGxraZP8AzRnfG5hMt+oth4FfZx8wDCicCgYEAtRFkQatBQqtHALJV+vgX
4D9RI88Opf111gxsKBBwDKLGI9jM7+P9Prsh5khzS86K28SemTBzq2OQFICxLdk5
bL6cOgcBw5+q33Qoc+ZV7fwZU3MOsvP/DSLzhi8OrUyOAbgneG1EpniX8FgkW6Uy
ocwAiKOnjorUn5QaySglJ38=
-----END PRIVATE KEY-----"#;

const MODULUS: &str = "ufZ-pnKWZ2mWZH0ymg1xbKAbUEvGD4PY0gSNNN0IWe5LUwCwqVUO9L3DQAzOdUvH_h39Tm_IDS7nwgpsNmyMyLH7mTq7mmhAbtSccalNo0Cfe0kFWDXmTkkZoAhBcSB8-XckzcO_LvnmOUp5mJ9XytQCJH4myTIpCnRE_Y5ofGNrX_vbjNLPvbtiw6KIdjn_Lci5fy37bh1867i2FH1RQtW6GRZpPehJZsJ66Zujcz_oC4aRpFp-1WCeh17g5az0aJmvXpt8-VzFZBxcJvzzRI_P5tJY7dfzCSd_M8Tl35Dbshr7uGgZe1u24nK4BPAOIC1nRq_fHMl76VAXIp3BVw";

#[derive(Clone, Serialize)]
struct TestClaims {
    iss: String,
    sub: String,
    exp: i64,
    aud: String,
    nonce: String,
}

fn token(claims: &TestClaims) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("life-test-key".into());
    encode(
        &header,
        claims,
        &EncodingKey::from_rsa_pem(RSA_PRIVATE_KEY.as_bytes()).expect("test RSA key"),
    )
    .expect("encode test JWT")
}

#[tokio::test]
async fn validates_signature_issuer_audience_expiry_subject_and_nonce() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind OIDC fixture");
    let issuer = format!("http://{}", listener.local_addr().expect("OIDC address"));
    let jwks_uri = format!("{issuer}/jwks");
    let discovery_issuer = issuer.clone();
    let app = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(move || {
                let jwks_uri = jwks_uri.clone();
                let issuer = discovery_issuer.clone();
                async move { Json(json!({"issuer": issuer, "jwks_uri": jwks_uri})) }
            }),
        )
        .route(
            "/jwks",
            get(|| async {
                Json(json!({"keys":[{
                    "kty":"RSA", "use":"sig", "kid":"life-test-key", "alg":"RS256",
                    "n":MODULUS, "e":"AQAB"
                }]}))
            }),
        );
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve OIDC fixture");
    });

    let verifier = OidcVerifier::new(&issuer, "life-workbench").expect("verifier");
    let valid = TestClaims {
        iss: issuer.clone(),
        sub: "life-user-subject".into(),
        exp: Utc::now().timestamp() + 300,
        aud: "life-workbench".into(),
        nonce: "oidc-nonce-123".into(),
    };
    let verified = verifier.verify(&token(&valid), "oidc-nonce-123").await;
    assert!(verified.is_ok(), "valid token rejected: {verified:?}");

    let mut bad_signature = token(&valid);
    let final_byte = bad_signature.pop().expect("JWT byte");
    bad_signature.push(if final_byte == 'A' { 'B' } else { 'A' });
    assert!(verifier
        .verify(&bad_signature, "oidc-nonce-123")
        .await
        .is_err());

    for invalid in [
        TestClaims {
            iss: "https://wrong-issuer.invalid".into(),
            ..valid.clone()
        },
        TestClaims {
            aud: "wrong-audience".into(),
            ..valid.clone()
        },
        TestClaims {
            exp: Utc::now().timestamp() - 60,
            ..valid.clone()
        },
        TestClaims {
            sub: String::new(),
            ..valid.clone()
        },
        TestClaims {
            nonce: "wrong-nonce".into(),
            ..valid.clone()
        },
    ] {
        assert!(verifier
            .verify(&token(&invalid), "oidc-nonce-123")
            .await
            .is_err());
    }
}
