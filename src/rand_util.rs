use rand::{distributions::Alphanumeric, Rng};

/// Génère une chaîne alphanumérique aléatoire de la longueur spécifiée.
pub fn generate_random_string(length: usize) -> String {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(length)
        .map(|c| c as char)
        .collect()
}
