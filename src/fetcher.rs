use flate2::read::GzDecoder;
use std::{
    collections::BTreeSet,
    collections::hash_map::DefaultHasher,
    error::Error,
    fs,
    hash::{Hash, Hasher},
    io::{Cursor, Read},
    path::PathBuf,
    time::{Duration, SystemTime},
};

/// Récupère un guide XML depuis une URL (HTTP ou HTTPS).
pub fn fetch(url: &str) -> Result<String, Box<dyn Error>> {
    load_guide(url, false).map(|(xml, _)| xml)
}

/// Vérifie la source en ligne et indique si le contenu du cache a changé.
pub fn refresh(url: &str) -> Result<(String, bool), Box<dyn Error>> {
    let (xml, changed) = load_guide(url, true)?;
    Ok((xml, changed.unwrap_or(false)))
}

fn load_guide(url: &str, force_refresh: bool) -> Result<(String, Option<bool>), Box<dyn Error>> {
    let cache_file = cache_path(url)?;
    if !force_refresh && cache_is_fresh(&cache_file) {
        return Ok((fs::read_to_string(cache_file)?, None));
    }

    let response = reqwest::blocking::Client::builder()
        .user_agent(concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
        .build()?
        .get(url)
        .send()?
        .error_for_status()?;
    let compressed = response.bytes()?;

    let xml = unpack_guide(&compressed, url)?;
    let xml = String::from_utf8(xml)?;

    if fs::read_to_string(&cache_file).is_ok_and(|cached| cached == xml) {
        return Ok((xml, Some(false)));
    }

    if let Some(parent) = cache_file.parent() {
        fs::create_dir_all(parent)?;
    }
    // Écriture temporaire pour éviter de laisser un cache partiel si le
    // processus s'arrête pendant la sauvegarde.
    let temporary_file = cache_file.with_extension("xml.tmp");
    fs::write(&temporary_file, &xml)?;
    #[cfg(windows)]
    if cache_file.exists() {
        fs::remove_file(&cache_file)?;
    }
    fs::rename(temporary_file, cache_file)?;

    Ok((xml, Some(true)))
}

/// Date de modification du dernier guide XML enregistré dans le cache.
pub fn cached_modified_at(url: &str) -> Option<SystemTime> {
    let cache_file = cache_path(url).ok()?;
    fs::metadata(cache_file).ok()?.modified().ok()
}

/// Emplacement stable du cache des images associées à un guide.
pub fn image_cache_path(url: &str) -> Result<PathBuf, Box<dyn Error>> {
    let guide_cache = cache_path(url)?;
    let directory = guide_cache.parent().ok_or("Chemin de cache invalide")?.join("images");
    let name = guide_cache.file_stem().ok_or("Nom de cache invalide")?;
    Ok(directory.join(format!("{}.img", name.to_string_lossy())))
}

/// Chaînes que l'utilisateur a choisi de retirer de la liste des programmes.
pub fn load_hidden_channels() -> Result<BTreeSet<String>, Box<dyn Error>> {
    let path = preferences_path()?;
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    Ok(fs::read_to_string(path)?
        .lines()
        .map(str::trim)
        .filter(|channel| !channel.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Enregistre les chaînes masquées de façon atomique pour les prochains lancements.
pub fn save_hidden_channels(channels: &BTreeSet<String>) -> Result<(), Box<dyn Error>> {
    let path = preferences_path()?;
    let directory = path.parent().ok_or("Chemin de préférences invalide")?;
    fs::create_dir_all(directory)?;
    let temporary = path.with_extension("txt.tmp");
    fs::write(&temporary, channels.iter().map(String::as_str).collect::<Vec<_>>().join("\n"))?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(&path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

/// Indique si les chaînes masquées doivent aussi être cachées dans la barre des chaînes.
pub fn load_hide_hidden_channel_logos() -> bool {
    hide_logos_path().map(|path| path.exists()).unwrap_or(false)
}

/// Mémorise l'état du bouton « Cacher chaînes masquées » pour les prochains lancements.
pub fn save_hide_hidden_channel_logos(hide: bool) -> Result<(), Box<dyn Error>> {
    let path = hide_logos_path()?;
    if hide {
        fs::create_dir_all(path.parent().ok_or("Chemin de préférences invalide")?)?;
        fs::write(path, "1")?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn unpack_guide(bytes: &[u8], url: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if url.to_ascii_lowercase().ends_with(".zip") || bytes.starts_with(b"PK\x03\x04") {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name().to_ascii_lowercase();
            if entry.is_dir() || !(name.ends_with(".xml") || name.ends_with(".xml.gz")) {
                continue;
            }

            let mut contents = Vec::new();
            entry.read_to_end(&mut contents)?;
            if name.ends_with(".gz") || contents.starts_with(&[0x1f, 0x8b]) {
                let mut decoder = GzDecoder::new(contents.as_slice());
                let mut xml = Vec::new();
                decoder.read_to_end(&mut xml)?;
                return Ok(xml);
            }
            return Ok(contents);
        }
        return Err("Aucun fichier XML ou XML.GZ trouvé dans l'archive ZIP".into());
    }

    if url.to_ascii_lowercase().ends_with(".gz") || bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = GzDecoder::new(bytes);
        let mut xml = Vec::new();
        decoder.read_to_end(&mut xml)?;
        return Ok(xml);
    }

    Ok(bytes.to_vec())
}

fn cache_path(url: &str) -> Result<PathBuf, Box<dyn Error>> {
    let mut directory = cache_directory()?;
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    directory.push(format!("{:016x}.xml", hasher.finish()));
    Ok(directory)
}

fn preferences_path() -> Result<PathBuf, Box<dyn Error>> {
    Ok(cache_directory()?.join("hidden_channels.txt"))
}

fn hide_logos_path() -> Result<PathBuf, Box<dyn Error>> {
    Ok(cache_directory()?.join("hide_hidden_channels"))
}

fn cache_directory() -> Result<PathBuf, Box<dyn Error>> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or("Ni HOME ni USERPROFILE ne sont définies")?;
    Ok(PathBuf::from(home).join(".cache/tnt_guide"))
}

fn cache_is_fresh(path: &PathBuf) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age < Duration::from_secs(12 * 60 * 60))
}
