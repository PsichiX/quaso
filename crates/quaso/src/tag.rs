use std::{
    borrow::Cow,
    collections::HashSet,
    hash::{Hash, Hasher},
    ops::{Deref, DerefMut, Range},
    str::FromStr,
};

#[derive(Default, Clone)]
pub struct Tag {
    content: Cow<'static, str>,
    parts: Vec<Range<usize>>,
}

impl Tag {
    pub fn new(content: impl Into<Cow<'static, str>>) -> Self {
        let content = content.into();
        let mut range = 0..0;
        let parts = content
            .chars()
            .chain(std::iter::once('.'))
            .filter_map(|character| {
                let result = range.clone();
                range.end += character.len_utf8();
                if character == '.' {
                    range.start = range.end;
                    Some(result)
                } else {
                    None
                }
            })
            .collect();
        Self { content, parts }
    }

    pub fn as_str(&self) -> &str {
        &self.content
    }

    pub fn parts(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().map(|range| &self.content[range.clone()])
    }

    pub fn part(&self, index: usize) -> Option<&str> {
        self.parts
            .get(index)
            .map(|range| &self.content[range.clone()])
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub fn fragment(&self, mut parts: usize) -> &str {
        parts = parts.min(self.len()).saturating_sub(1);
        &self.content[0..(self.parts[parts].end)]
    }

    pub fn parent_fragment(&self) -> &str {
        self.fragment(self.len().saturating_sub(1))
    }

    pub fn sub_tag(&self, parts: usize) -> Self {
        Self::new(self.fragment(parts).to_owned())
    }

    pub fn parent(&self) -> Self {
        Self::new(self.parent_fragment().to_owned())
    }

    pub fn push(&self, part: &str) -> Self {
        Self::new(format!("{}.{}", self.content, part))
    }

    pub fn shared_parts(&self, other: &Self) -> usize {
        let mut result = 0;
        for (a, b) in self.parts().zip(other.parts()) {
            if a != b {
                return result;
            }
            result += 1;
        }
        result
    }

    pub fn is_subset_of(&self, other: &Self) -> bool {
        self.len() <= other.len() && self.shared_parts(other) == self.len()
    }

    pub fn is_superset_of(&self, other: &Self) -> bool {
        self.len() >= other.len() && self.shared_parts(other) == other.len()
    }

    pub fn matches(&self, other: &Self) -> bool {
        if self.len() > other.len() {
            return false;
        }
        for (a, b) in self.parts().zip(other.parts()) {
            if a != "*" && a != b {
                return false;
            }
        }
        true
    }
}

impl Hash for Tag {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.content.hash(state);
    }
}

impl PartialEq for Tag {
    fn eq(&self, other: &Self) -> bool {
        self.content == other.content
    }
}

impl PartialOrd for Tag {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tag {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.content.cmp(&other.content)
    }
}

impl Eq for Tag {}

impl std::fmt::Debug for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tag")
            .field("content", &self.content)
            .field(
                "parts",
                &self
                    .parts
                    .iter()
                    .map(|range| &self.content[range.clone()])
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.content)
    }
}

impl FromStr for Tag {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s.to_owned()))
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tags(HashSet<Tag>);

impl Tags {
    pub fn with(mut self, tag: impl Into<Tag>) -> Self {
        self.0.insert(tag.into());
        self
    }

    pub fn matching_any_iter(&self, other: &Self) -> impl Iterator<Item = &Tag> {
        self.0.iter().filter(move |tag| other.matches_any(tag))
    }

    pub fn matching_all_iter(&self, other: &Self) -> impl Iterator<Item = &Tag> {
        self.0.iter().filter(move |tag| other.matches_all(tag))
    }

    pub fn matches_any(&self, tag: &Tag) -> bool {
        self.0.iter().any(|t| t.matches(tag))
    }

    pub fn matches_all(&self, tag: &Tag) -> bool {
        self.0.iter().all(|t| t.matches(tag))
    }

    pub fn matching_union_any(&self, other: &Self) -> Self {
        Self(
            self.0
                .iter()
                .filter(|tag| other.matches_any(tag))
                .cloned()
                .collect(),
        )
    }

    pub fn matching_union_all(&self, other: &Self) -> Self {
        Self(
            self.0
                .iter()
                .filter(|tag| other.matches_all(tag))
                .cloned()
                .collect(),
        )
    }

    pub fn matching_difference_any(&self, other: &Self) -> Self {
        Self(
            self.0
                .iter()
                .filter(|tag| !other.matches_any(tag))
                .cloned()
                .collect(),
        )
    }

    pub fn matching_difference_all(&self, other: &Self) -> Self {
        Self(
            self.0
                .iter()
                .filter(|tag| !other.matches_all(tag))
                .cloned()
                .collect(),
        )
    }
}

impl From<HashSet<Tag>> for Tags {
    fn from(value: HashSet<Tag>) -> Self {
        Self(value)
    }
}

impl FromIterator<Tag> for Tags {
    fn from_iter<T: IntoIterator<Item = Tag>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for Tags {
    type Item = Tag;
    type IntoIter = std::collections::hash_set::IntoIter<Tag>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Deref for Tags {
    type Target = HashSet<Tag>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Tags {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::Tag;

    #[test]
    fn test_tag() {
        let tag = Tag::new("abra.ca.dabra");
        assert_eq!(tag.fragment(0), "abra");
        assert_eq!(tag.fragment(1), "abra");
        assert_eq!(tag.fragment(2), "abra.ca");
        assert_eq!(tag.fragment(3), "abra.ca.dabra");
        assert_eq!(tag.fragment(4), "abra.ca.dabra");
        assert_eq!(tag.as_str(), "abra.ca.dabra");
        assert_eq!(tag, tag.as_str().parse::<Tag>().unwrap());
        assert_eq!(tag.parent_fragment(), "abra.ca");
        assert_eq!(tag.parent(), Tag::new("abra.ca"));
        assert_eq!(tag.parent().parent(), Tag::new("abra"));
        assert_eq!(tag.parent().push("foo"), Tag::new("abra.ca.foo"));
        assert_eq!(tag.shared_parts(&Tag::new("abra.ca.foo")), 2);
        assert_eq!(tag.shared_parts(&Tag::new("abra.ca")), 2);
        assert_eq!(tag.shared_parts(&Tag::new("abra.foo")), 1);
        assert_eq!(tag.shared_parts(&Tag::new("foo")), 0);
        assert_eq!(tag.shared_parts(&Tag::new("abra.ca.dabra.foo")), 3);
        assert!(tag.is_subset_of(&Tag::new("abra.ca.dabra")));
        assert!(tag.is_subset_of(&Tag::new("abra.ca.dabra.foo")));
        assert!(!tag.is_subset_of(&Tag::new("abra.ca")));
        assert!(!tag.is_subset_of(&Tag::new("abra.ca.foo")));
        assert!(tag.is_superset_of(&Tag::new("abra.ca.dabra")));
        assert!(!tag.is_superset_of(&Tag::new("abra.ca.dabra.foo")));
        assert!(tag.is_superset_of(&Tag::new("abra.ca")));
        assert!(!tag.is_superset_of(&Tag::new("abra.ca.foo")));
        assert!(Tag::new("abra.ca.dabra").matches(&tag));
        assert!(!Tag::new("abra.ca.dabra.foo").matches(&tag));
        assert!(Tag::new("abra.ca").matches(&tag));
        assert!(!Tag::new("abra.ca.foo").matches(&tag));
        assert!(Tag::new("abra").matches(&tag));
        assert!(!Tag::new("abra.foo").matches(&tag));
        assert!(!Tag::new("foo").matches(&tag));
        assert!(Tag::new("abra.ca.*").matches(&tag));
        assert!(!Tag::new("foo.ca.*").matches(&tag));
        assert!(Tag::new("abra.*.dabra").matches(&tag));
        assert!(!Tag::new("abra.*.foo").matches(&tag));
        assert!(Tag::new("*.ca.dabra").matches(&tag));
        assert!(!Tag::new("*.foo.dabra").matches(&tag));
    }
}
