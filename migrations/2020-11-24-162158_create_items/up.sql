CREATE TABLE metadbs (
  id INTEGER PRIMARY KEY NOT NULL,
  location TEXT NOT NULL,
  subsong INT NOT NULL,
  mimetype TEXT
);

CREATE TABLE tags (
  id INTEGER NOT NULL,
  key TEXT NOT NULL,
  value TEXT NOT NULL,

  PRIMARY KEY(id, key)
);