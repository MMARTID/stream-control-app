//! Certificado HTTPS local del panel (RF-48) y utilidades de red local (RF-44).
//!
//! El certificado es autofirmado: el navegador del móvil avisará la primera vez en cada
//! dispositivo, como dice el README. La clave privada se guarda en el almacén seguro del
//! sistema, no en un archivo; el certificado (público) va en un archivo junto a los ajustes.
//! Si cambia la IP del PC y el certificado no la cubre, se genera uno nuevo.

use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    path::Path,
};

use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, date_time_ymd};

use crate::settings::secrets::{PANEL_TLS_KEY, SecretStore};

/// ¿Es una dirección de la red local? Solo estas pueden usar el panel (RF-44).
pub fn is_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_lan(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// Direcciones IPv4 privadas de este equipo (las que ve el móvil en la misma Wi‑Fi).
pub fn local_ips() -> Vec<Ipv4Addr> {
    let mut ips: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(v4) if v4.is_private() => Some(v4),
            _ => None,
        })
        .collect();
    ips.sort();
    ips.dedup();
    ips
}

pub fn hostname() -> Option<String> {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|var| std::env::var(var).ok())
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'))
}

pub struct Identity {
    pub cert_pem: String,
    pub key_pem: String,
}

fn wanted_names(ips: &[Ipv4Addr], hostname: Option<&str>) -> Vec<String> {
    let mut names = vec!["localhost".to_owned(), "127.0.0.1".to_owned()];
    names.extend(ips.iter().map(|ip| ip.to_string()));
    if let Some(host) = hostname {
        names.push(host.to_lowercase());
        if !host.contains('.') {
            names.push(format!("{}.local", host.to_lowercase()));
        }
    }
    names
}

/// Devuelve el certificado y la clave, reutilizando los guardados mientras cubran las direcciones actuales.
pub fn ensure_identity(
    dir: &Path,
    secrets: &dyn SecretStore,
    ips: &[Ipv4Addr],
    hostname: Option<&str>,
) -> Result<Identity, String> {
    let cert_path = dir.join("panel-cert.pem");
    let names_path = dir.join("panel-cert.names");
    let wanted = wanted_names(ips, hostname);

    if let (Ok(cert_pem), Ok(names), Ok(Some(key_pem))) =
        (fs::read_to_string(&cert_path), fs::read_to_string(&names_path), secrets.get(PANEL_TLS_KEY))
    {
        let covered: Vec<&str> = names.lines().collect();
        if wanted.iter().all(|w| covered.contains(&w.as_str())) {
            return Ok(Identity { cert_pem, key_pem });
        }
    }

    let mut params = CertificateParams::new(wanted.clone()).map_err(|e| format!("No se pudo preparar el certificado: {e}"))?;
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "Stream Control");
    params.distinguished_name = name;
    // Sin dependencia del reloj del equipo: válido de sobra para el uso en casa.
    params.not_before = date_time_ymd(2024, 1, 1);
    params.not_after = date_time_ymd(2049, 1, 1);
    let key = KeyPair::generate().map_err(|e| format!("No se pudo generar la clave del certificado: {e}"))?;
    let cert = params.self_signed(&key).map_err(|e| format!("No se pudo firmar el certificado: {e}"))?;
    let identity = Identity { cert_pem: cert.pem(), key_pem: key.serialize_pem() };

    // Si no se puede guardar, el panel funciona igualmente con un certificado de esta sesión.
    let _ = fs::create_dir_all(dir);
    if secrets.set(PANEL_TLS_KEY, &identity.key_pem).is_ok() {
        let _ = fs::write(&cert_path, &identity.cert_pem);
        let _ = fs::write(&names_path, wanted.join("\n"));
    }
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::secrets::MemoryStore;

    #[test]
    fn solo_se_aceptan_direcciones_de_la_red_local() {
        for ok in ["127.0.0.1", "10.0.0.5", "172.16.3.4", "172.31.255.255", "192.168.1.20", "169.254.1.1", "::1", "fd12:3456::1", "fe80::1", "::ffff:192.168.1.5"] {
            assert!(is_lan(ok.parse().unwrap()), "{ok} debería ser local");
        }
        for bad in ["8.8.8.8", "172.32.0.1", "100.64.0.1", "1.1.1.1", "2606:4700::1111", "::ffff:8.8.8.8"] {
            assert!(!is_lan(bad.parse().unwrap()), "{bad} NO debería ser local");
        }
    }

    #[test]
    fn el_certificado_se_reutiliza_y_se_regenera_si_cambia_la_ip() {
        let dir = std::env::temp_dir().join(format!("stream-control-tls-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let secrets = MemoryStore::default();
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();

        let first = ensure_identity(&dir, &secrets, &[ip("192.168.1.20")], Some("MI-PC")).unwrap();
        assert!(first.cert_pem.contains("BEGIN CERTIFICATE") && first.key_pem.contains("PRIVATE KEY"));
        assert!(!dir.join("panel-cert.pem").to_string_lossy().is_empty());
        // La clave privada no está en ningún archivo.
        for entry in fs::read_dir(&dir).unwrap() {
            assert!(!fs::read_to_string(entry.unwrap().path()).unwrap().contains("PRIVATE KEY"));
        }

        let same = ensure_identity(&dir, &secrets, &[ip("192.168.1.20")], Some("MI-PC")).unwrap();
        assert_eq!(same.cert_pem, first.cert_pem);

        let moved = ensure_identity(&dir, &secrets, &[ip("192.168.1.77")], Some("MI-PC")).unwrap();
        assert_ne!(moved.cert_pem, first.cert_pem);
    }
}
