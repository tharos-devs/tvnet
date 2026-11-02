use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone};
use quick_xml::{Reader, events::Event, escape::unescape};
use std::{collections::HashMap, error::Error};

#[derive(Debug, Clone)]
pub struct Film {
    /// Nom de la chaîne qui diffuse le film.
    pub chaine: String,
    pub logo_url: Option<String>,
    pub titre: String,
    pub debut: DateTime<FixedOffset>,
    pub fin: DateTime<FixedOffset>,
    pub resume: Option<String>,
    pub acteurs: Vec<String>,
    pub annee: Option<String>,
    pub pays: Option<String>,
    pub genres: Vec<String>,
    pub affiche_url: Option<String>,
}

#[derive(Debug, Default)]
struct Chaine {
    nom: String,
    logo_url: Option<String>,
}

#[derive(Debug, Default)]
struct ProgrammeBuilder {
    chaine_id: String,
    titre: String,
    resume: Option<String>,
    acteurs: Vec<String>,
    annee: Option<String>,
    pays: Option<String>,
    affiche_url: Option<String>,
    categories: Vec<String>,
    debut: Option<DateTime<FixedOffset>>,
    fin: Option<DateTime<FixedOffset>>,
}

/// Parcourt un document XMLTV, associe les programmes aux chaînes et renvoie
/// les films de la date demandée qui chevauchent la plage 20 h–23 h 30.
pub fn parse_evening_movies(xml: &str, date: NaiveDate) -> Result<Vec<Film>, Box<dyn Error>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut chaines: HashMap<String, Chaine> = HashMap::new();
    let mut films = Vec::new();
    let mut chaine_courante: Option<(String, Chaine)> = None;
    let mut programme_courant: Option<ProgrammeBuilder> = None;
    let mut champ: Option<&'static str> = None;
    let mut dans_credits = false;

    loop {
        match reader.read_event()? {
            Event::Start(event) if event.name().as_ref() == b"channel" => {
                let id = attribute(&event, b"id")?.unwrap_or_default();
                chaine_courante = Some((id, Chaine::default()));
            }
            Event::Empty(event) if event.name().as_ref() == b"icon" => {
                if let Some(src) = attribute(&event, b"src")? {
                    if let Some((_, chaine)) = chaine_courante.as_mut() {
                        chaine.logo_url = Some(src.clone());
                    } else if let Some(programme) = programme_courant.as_mut() {
                        programme.affiche_url = Some(src);
                    }
                }
            }
            Event::Start(event) if event.name().as_ref() == b"icon" => {
                if let Some(src) = attribute(&event, b"src")? {
                    if let Some((_, chaine)) = chaine_courante.as_mut() {
                        chaine.logo_url = Some(src);
                    } else if let Some(programme) = programme_courant.as_mut() {
                        programme.affiche_url = Some(src);
                    }
                }
            }
            Event::Start(event) if event.name().as_ref() == b"programme" => {
                let mut programme = ProgrammeBuilder {
                    chaine_id: attribute(&event, b"channel")?.unwrap_or_default(),
                    ..ProgrammeBuilder::default()
                };
                if let Some(value) = attribute(&event, b"start")? {
                    programme.debut = Some(parse_xmltv_time(&value)?);
                }
                if let Some(value) = attribute(&event, b"stop")? {
                    programme.fin = Some(parse_xmltv_time(&value)?);
                }
                programme_courant = Some(programme);
            }
            Event::Start(event) => {
                let name = event.name();
                let name = name.as_ref();
                champ = match name {
                    b"display-name" if chaine_courante.is_some() => Some("display-name"),
                    b"title" if programme_courant.is_some() => Some("title"),
                    b"desc" if programme_courant.is_some() => Some("desc"),
                    b"category" if programme_courant.is_some() => Some("category"),
                    b"date" if programme_courant.is_some() => Some("date"),
                    b"country" if programme_courant.is_some() => Some("country"),
                    b"actor" if programme_courant.is_some() && dans_credits => Some("actor"),
                    b"credits" => {
                        dans_credits = true;
                        None
                    }
                    _ => None,
                };
            }
            Event::Text(event) => {
                if let Some(champ_actif) = champ {
                    let decoded = reader.decoder().decode(event.as_ref())?;
                    let value = unescape(&decoded)?.into_owned();
                    match champ_actif {
                        "display-name" => {
                            if let Some((_, chaine)) = chaine_courante.as_mut() {
                                chaine.nom.push_str(&value);
                            }
                        }
                        "title" => {
                            if let Some(programme) = programme_courant.as_mut() {
                                programme.titre.push_str(&value);
                            }
                        }
                        "desc" => {
                            if let Some(programme) = programme_courant.as_mut() {
                                programme.resume.get_or_insert_with(String::new).push_str(&value);
                            }
                        }
                        "category" => {
                            if let Some(programme) = programme_courant.as_mut() {
                                programme.categories.push(value);
                            }
                        }
                        "date" => {
                            if let Some(programme) = programme_courant.as_mut() {
                                let year = value.chars().filter(char::is_ascii_digit).take(4).collect::<String>();
                                if year.len() == 4 { programme.annee = Some(year); }
                            }
                        }
                        "country" => {
                            if let Some(programme) = programme_courant.as_mut() {
                                programme.pays.get_or_insert_with(String::new).push_str(&value);
                            }
                        }
                        "actor" => {
                            if let Some(programme) = programme_courant.as_mut() {
                                programme.acteurs.push(value);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::CData(event) => {
                if champ == Some("title") {
                    if let Some(programme) = programme_courant.as_mut() {
                        programme.titre.push_str(&event.decode()?);
                    }
                } else if champ == Some("desc") {
                    if let Some(programme) = programme_courant.as_mut() {
                        programme.resume.get_or_insert_with(String::new).push_str(&event.decode()?);
                    }
                }
            }
            Event::End(event) => match event.name().as_ref() {
                b"display-name" | b"title" | b"desc" | b"category" | b"date" | b"country" | b"actor" => champ = None,
                b"credits" => dans_credits = false,
                b"channel" => {
                    if let Some((id, chaine)) = chaine_courante.take() {
                        chaines.insert(id, chaine);
                    }
                    champ = None;
                }
                b"programme" => {
                    if let Some(programme) = programme_courant.take() {
                        if let (Some(debut), Some(fin)) = (programme.debut, programme.fin) {
                            if is_evening_overlap(debut, fin, date)
                                && programme.categories.iter().any(|category| is_film_category(category))
                            {
                                let chaine = chaines.get(&programme.chaine_id);
                                films.push(Film {
                                    chaine: chaine
                                        .map(|chaine| chaine.nom.clone())
                                        .filter(|nom| !nom.is_empty())
                                        .unwrap_or_else(|| programme.chaine_id.clone()),
                                    logo_url: chaine.and_then(|chaine| chaine.logo_url.clone()),
                                    titre: programme.titre,
                                    debut,
                                    fin,
                                    resume: programme.resume,
                                    acteurs: programme.acteurs,
                                    annee: programme.annee,
                                    pays: programme.pays,
                                    genres: programme.categories,
                                    affiche_url: programme.affiche_url,
                                });
                            }
                        }
                    }
                    champ = None;
                    dans_credits = false;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    films.sort_by_key(|film| film.debut);
    Ok(films)
}

fn attribute(
    event: &quick_xml::events::BytesStart<'_>,
    key: &[u8],
) -> Result<Option<String>, Box<dyn Error>> {
    for attribute in event.attributes() {
        let attribute = attribute?;
        if attribute.key.as_ref() == key {
            return Ok(Some(attribute.unescape_value()?.into_owned()));
        }
    }
    Ok(None)
}

fn is_evening_overlap(debut: DateTime<FixedOffset>, fin: DateTime<FixedOffset>, date: NaiveDate) -> bool {
    let Some(start_of_evening) = date.and_hms_opt(20, 0, 0) else { return false };
    let Some(end_of_evening) = date.and_hms_opt(23, 30, 0) else { return false };
    let Some(start_of_evening) = debut.offset().from_local_datetime(&start_of_evening).single() else { return false };
    let Some(end_of_evening) = debut.offset().from_local_datetime(&end_of_evening).single() else { return false };
    debut < end_of_evening && fin > start_of_evening
}

fn is_film_category(category: &str) -> bool {
    let category = category.to_lowercase();
    [
        "film", "cinéma", "cinema", "téléfilm", "telefilm", "fiction", "comédie", "comedie",
        "drame", "thriller", "policier", "action", "aventure", "fantastique", "horreur",
        "animation", "western", "biographie", "historique", "guerre", "romance", "romantique",
        "science-fiction", "science fiction",
    ]
    .iter()
    .any(|genre| category.contains(genre))
}

fn parse_xmltv_time(value: &str) -> Result<DateTime<FixedOffset>, Box<dyn Error>> {
    let (timestamp, offset) = value.split_once(' ').unwrap_or((value, "+0000"));
    let local = NaiveDateTime::parse_from_str(timestamp, "%Y%m%d%H%M%S")?;
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let digits = offset.trim_start_matches(['+', '-']);
    let hours: i32 = digits.get(..2).unwrap_or("00").parse()?;
    let minutes: i32 = digits.get(2..4).unwrap_or("00").parse()?;
    let fixed = FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))
        .ok_or("Décalage horaire XMLTV invalide")?;
    Ok(DateTime::from_naive_utc_and_offset(local - fixed, fixed))
}
