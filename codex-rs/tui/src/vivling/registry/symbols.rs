use super::super::model::Stage;
use super::types::VivlingRarity;

pub(super) fn species_variant(id: &str, name: &str, stage: Stage) -> usize {
    let stage_salt = match stage {
        Stage::Baby => 11usize,
        Stage::Juvenile => 23usize,
        Stage::Adult => 37usize,
    };
    id.bytes()
        .chain(name.bytes())
        .fold(stage_salt, |acc, byte| {
            acc.wrapping_mul(33).wrapping_add(byte as usize)
        })
}

pub(super) fn variant_symbol(variant: usize) -> char {
    ['^', '*', '~', '+', 'o', '#', '%', '='][variant % 8]
}

pub(super) fn eye_symbol(variant: usize) -> char {
    ['.', '\'', ':', '*', 'o', ';', '`', '+'][variant % 8]
}

pub(super) fn rarity_badge(rarity: VivlingRarity, variant: usize) -> char {
    let common = ['.', '\'', '~', ','];
    let rare = ['*', '+', '^', '!'];
    let legendary = ['#', '@', '$', '%'];
    let mythic = ['>', '<', '|', '0'];
    match rarity {
        VivlingRarity::Common => common[variant % 4],
        VivlingRarity::Rare => rare[variant % 4],
        VivlingRarity::Legendary => legendary[variant % 4],
        VivlingRarity::Mythic => mythic[variant % 4],
    }
}

pub(super) fn species_mark(id: &str, name: &str) -> String {
    let mut id_letters = id.chars().filter(|ch| ch.is_ascii_alphabetic());
    let first = id_letters.next().unwrap_or('x');
    let second = id_letters.next().unwrap_or(first);
    let third = name
        .chars()
        .rev()
        .find(|ch| ch.is_ascii_alphabetic())
        .unwrap_or('x');
    format!(
        "{}{}{}",
        first.to_ascii_uppercase(),
        second.to_ascii_uppercase(),
        third.to_ascii_uppercase()
    )
}
