-- pacod-aggregator's database.

-- One row: the settings an administrator changes in the console, as a document with a version.
CREATE TABLE settings (
    id      boolean PRIMARY KEY DEFAULT true CHECK (id),
    doc     jsonb NOT NULL DEFAULT '{}'::jsonb,
    version integer NOT NULL DEFAULT 0
);
INSERT INTO settings DEFAULT VALUES;
