use super::schema::metadbs;
use super::schema::tags;

#[derive(Queryable)]
pub struct Metadb {
    pub id: i32,
    pub location: String,
    pub subsong: i32,
    pub mimetype: Option<String>,
}

#[derive(Queryable)]
pub struct Tag {
    pub id: i32,
    pub key: String,
    pub value: String,
}

#[derive(Insertable)]
#[table_name="metadbs"]
pub struct NewMetadb<'a> {
    pub location: &'a str,
    pub subsong: &'a i32,
    pub mimetype: &'a str,
}

#[derive(Insertable)]
#[table_name="tags"]
pub struct NewTag<'a> {
    pub id: &'a i32,
    pub key: &'a str,
    pub value: &'a str,
}