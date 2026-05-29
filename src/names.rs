const ADJECTIVES: &[&str] = &[
    // Texture & physical
    "bright",
    "burnished",
    "crisp",
    "dappled",
    "dusty",
    "flaky",
    "frosted",
    "fuzzy",
    "gilded",
    "glossy",
    "gnarled",
    "hazy",
    "jagged",
    "knobby",
    "lush",
    "mossy",
    "polished",
    "prickly",
    "rough",
    "rusty",
    "silky",
    "smooth",
    "speckled",
    "tangled",
    "thorny",
    "velvety",
    "weathered",
    "woven",
    // Mood & personality
    "bold",
    "brash",
    "brazen",
    "calm",
    "cheerful",
    "clever",
    "cozy",
    "daring",
    "defiant",
    "eager",
    "fierce",
    "gentle",
    "giddy",
    "grumpy",
    "hardy",
    "hasty",
    "jolly",
    "keen",
    "lazy",
    "lively",
    "mellow",
    "merry",
    "mighty",
    "noble",
    "patient",
    "plucky",
    "proud",
    "quiet",
    "rowdy",
    "serene",
    "shy",
    "sleepy",
    "smug",
    "snappy",
    "solemn",
    "steady",
    "stoic",
    "stubborn",
    "sulky",
    "swift",
    "tender",
    "timid",
    "vivid",
    "wandering",
    "wary",
    "wild",
    "wistful",
    "witty",
    "zealous",
    // Scale & intensity
    "ancient",
    "bitter",
    "brisk",
    "colossal",
    "dim",
    "electric",
    "faint",
    "fleeting",
    "grand",
    "hushed",
    "little",
    "luminous",
    "massive",
    "miniature",
    "radiant",
    "roaring",
    "secret",
    "shadowy",
    "silent",
    "slender",
    "smoldering",
    "stark",
    "subtle",
    "tiny",
    "towering",
    "twilight",
    "vast",
    // Color-adjacent
    "ashen",
    "azure",
    "copper",
    "crimson",
    "emerald",
    "golden",
    "ivory",
    "midnight",
    "scarlet",
    "silver",
    // Whimsy & weirdness
    "cosmic",
    "crooked",
    "feral",
    "forgotten",
    "ghostly",
    "hollow",
    "lost",
    "muddy",
    "phantom",
    "restless",
    "rickety",
    "soggy",
    "tattered",
    "twisted",
    "unlikely",
    "unruly",
    "wobbly",
];

const PLANTS: &[&str] = &[
    // Flowers
    "aster",
    "azalea",
    "begonia",
    "bluebell",
    "camellia",
    "carnation",
    "chrysanthemum",
    "clover",
    "columbine",
    "cornflower",
    "cosmos",
    "crocus",
    "daffodil",
    "dahlia",
    "daisy",
    "dandelion",
    "delphinium",
    "foxglove",
    "freesia",
    "gardenia",
    "geranium",
    "gladiolus",
    "hawthorn",
    "heather",
    "hibiscus",
    "hollyhock",
    "honeysuckle",
    "hyacinth",
    "hydrangea",
    "iris",
    "jasmine",
    "jonquil",
    "larkspur",
    "lavender",
    "lilac",
    "lily",
    "lotus",
    "lupin",
    "magnolia",
    "marigold",
    "morning-glory",
    "myrtle",
    "narcissus",
    "orchid",
    "pansy",
    "peony",
    "periwinkle",
    "petunia",
    "poppy",
    "primrose",
    "protea",
    "ranunculus",
    "rhododendron",
    "rose",
    "snapdragon",
    "sunflower",
    "sweetpea",
    "thistle",
    "tulip",
    "verbena",
    "violet",
    "wisteria",
    "zinnia",
    // Herbs & aromatics
    "basil",
    "chamomile",
    "cilantro",
    "dill",
    "fennel",
    "ginger",
    "lemongrass",
    "marjoram",
    "mint",
    "oregano",
    "parsley",
    "rosemary",
    "saffron",
    "sage",
    "tarragon",
    "thyme",
    // Trees & shrubs
    "acacia",
    "birch",
    "cedar",
    "cypress",
    "elder",
    "elm",
    "hazel",
    "hemlock",
    "holly",
    "juniper",
    "maple",
    "oak",
    "olive",
    "pine",
    "rowan",
    "spruce",
    "willow",
    "yew",
    // Ferns, mosses & other
    "bamboo",
    "bracken",
    "fern",
    "ivy",
    "lichen",
    "moss",
    "reed",
    "sorrel",
    "tansy",
    "woad",
];

use rand::RngExt;

pub fn generate() -> String {
    let mut rng = rand::rng();
    let a = ADJECTIVES[rng.random_range(0..ADJECTIVES.len())];
    let p = PLANTS[rng.random_range(0..PLANTS.len())];
    format!("{a}-{p}")
}

pub fn generate_from_seed(seed: u64) -> String {
    // Simple xorshift -> two indices. Deterministic.
    let mut x = seed.max(1);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let a_idx = (x as usize) % ADJECTIVES.len();
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let p_idx = (x as usize) % PLANTS.len();
    format!("{}-{}", ADJECTIVES[a_idx], PLANTS[p_idx])
}

/// True if `name` looks like a slug we generated (adj-plant from our wordlists).
/// Used as a guard: we only auto-rename workspaces whose name is still generated.
pub fn is_generated_slug(name: &str) -> bool {
    let (a, p) = match name.split_once('-') {
        Some(pair) => pair,
        None => return false,
    };
    ADJECTIVES.contains(&a) && PLANTS.contains(&p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_kebab_lowercase_ascii() {
        for _ in 0..50 {
            let n = generate();
            assert!(n.contains('-'));
            assert!(n.chars().all(|c| c.is_ascii_lowercase() || c == '-'));
        }
    }

    #[test]
    fn generate_from_seed_is_deterministic() {
        assert_eq!(generate_from_seed(42), generate_from_seed(42));
        assert_ne!(generate_from_seed(42), generate_from_seed(43));
    }

    #[test]
    fn detects_generated_slug() {
        // Pick a known pair from the wordlists.
        let example = generate_from_seed(42);
        assert!(is_generated_slug(&example));
        assert!(!is_generated_slug("fix-login-bug"));
        assert!(!is_generated_slug("single"));
        assert!(!is_generated_slug(""));
    }
}
