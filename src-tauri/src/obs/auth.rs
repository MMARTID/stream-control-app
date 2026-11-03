//! Cadena de autenticación de obs-websocket v5 (protocol.md, «Creating an authentication string»).

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

/// `base64(sha256(base64(sha256(password + salt)) + challenge))`
pub fn auth_string(password: &str, salt: &str, challenge: &str) -> String {
    let secret = STANDARD.encode(Sha256::digest(format!("{password}{salt}")));
    STANDARD.encode(Sha256::digest(format!("{secret}{challenge}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vector calculado de forma independiente con hashlib de Python, usando el
    /// salt y el challenge de ejemplo del protocolo y la contraseña `supersecretpassword`.
    #[test]
    fn coincide_con_el_vector_de_referencia() {
        let got = auth_string(
            "supersecretpassword",
            "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=",
            "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=",
        );
        assert_eq!(got, "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4=");
    }
}
