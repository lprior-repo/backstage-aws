use std::net::Ipv4Addr;

pub fn label(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value.starts_with(|ch: char| ch.is_ascii_alphanumeric())
        && value.ends_with(|ch: char| ch.is_ascii_alphanumeric())
}

pub fn region(value: &str) -> bool {
    (9..=24).contains(&value.len())
        && !value.starts_with("cn-")
        && !value.contains("-gov-")
        && value.split('-').count() >= 3
        && value.split('-').next().is_some_and(|part| part.len() == 2)
        && value.split('-').all(|part| label(part, 1, 16))
        && value
            .rsplit('-')
            .next()
            .is_some_and(|part| part.parse::<u16>().is_ok())
}

pub fn hostname(value: &str) -> bool {
    value.len() <= 253
        && value.contains('.')
        && value.parse::<Ipv4Addr>().is_err()
        && value.split('.').all(|part| label(part, 1, 63))
}

pub fn repository(value: &str) -> bool {
    value.split_once('/').is_some_and(|(owner, name)| {
        label(owner, 1, 39)
            && (1..=100).contains(&name.len())
            && ![".", ".."].contains(&name)
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    })
}

pub fn cidr(value: &str) -> bool {
    value.split_once('/').is_some_and(|(ip, prefix)| {
        ip.parse::<Ipv4Addr>().is_ok()
            && prefix.bytes().all(|byte| byte.is_ascii_digit())
            && prefix
                .parse::<u8>()
                .is_ok_and(|bits| (8..=32).contains(&bits))
    })
}

pub fn email(value: &str) -> bool {
    value.len() <= 254
        && value.split_once('@').is_some_and(|(local, domain)| {
            (1..=64).contains(&local.len())
                && local
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._%+-".contains(&byte))
                && hostname(domain)
        })
}
