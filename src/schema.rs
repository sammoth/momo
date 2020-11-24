table! {
    metadbs (id) {
        id -> Integer,
        location -> Text,
        subsong -> Integer,
        mimetype -> Nullable<Text>,
    }
}

table! {
    tags (id, key) {
        id -> Integer,
        key -> Text,
        value -> Text,
    }
}

allow_tables_to_appear_in_same_query!(
    metadbs,
    tags,
);
