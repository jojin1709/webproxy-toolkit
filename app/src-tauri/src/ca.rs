use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, IsCa,
    KeyPair, KeyUsagePurpose,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Ca {
    pub cert: Certificate,
    pub key_pair: KeyPair,
}

pub fn load_or_create(data_dir: &Path) -> Result<Arc<Ca>, String> {
    fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let cert_path = data_dir.join("ca.crt");
    let key_path = data_dir.join("ca.key");

    if cert_path.exists() && key_path.exists() {
        let cert_pem = fs::read_to_string(&cert_path).map_err(|e| e.to_string())?;
        let key_pem = fs::read_to_string(&key_path).map_err(|e| e.to_string())?;
        let key_pair = KeyPair::from_pem(&key_pem).map_err(|e| e.to_string())?;
        let params = CertificateParams::from_ca_cert_pem(&cert_pem).map_err(|e| e.to_string())?;
        let cert = params.self_signed(&key_pair).map_err(|e| e.to_string())?;
        return Ok(Arc::new(Ca { cert, key_pair }));
    }

    let mut params = CertificateParams::new(Vec::<String>::new()).map_err(|e| e.to_string())?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "WebProxy Toolkit Local CA");
    dn.push(DnType::OrganizationName, "WebProxy Toolkit");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.use_authority_key_identifier_extension = true;

    let key_pair = KeyPair::generate().map_err(|e| e.to_string())?;
    let cert = params.self_signed(&key_pair).map_err(|e| e.to_string())?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    fs::write(&cert_path, &cert_pem).map_err(|e| e.to_string())?;
    fs::write(&key_path, &key_pem).map_err(|e| e.to_string())?;

    Ok(Arc::new(Ca { cert, key_pair }))
}

pub fn ca_export_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ca.crt")
}

pub fn mint_leaf(ca: &Ca, host: &str) -> Result<(Certificate, KeyPair), String> {
    let params = CertificateParams::new(vec![host.to_string()]).map_err(|e| e.to_string())?;
    let key_pair = KeyPair::generate().map_err(|e| e.to_string())?;
    let cert = params
        .signed_by(&key_pair, &ca.cert, &ca.key_pair)
        .map_err(|e| e.to_string())?;
    Ok((cert, key_pair))
}
