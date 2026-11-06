-- pacod-aggregator's database.

-- One row: the settings an administrator changes in the console, as a document with a version.
CREATE TABLE settings (
    id      boolean PRIMARY KEY DEFAULT true CHECK (id),
    doc     jsonb NOT NULL DEFAULT '{}'::jsonb,
    version integer NOT NULL DEFAULT 0,
    -- The vendors a new installation started from, once each: one an administrator removed does
    -- not come back at the next start, and one a later release adds still arrives.
    seeded  text[] NOT NULL DEFAULT '{}'
);
INSERT INTO settings DEFAULT VALUES;

-- A company whose API serves models. Ids are the catalogue's (`pacod-agg-catalog`, `vendors.rs`):
-- a gateway maps its providers to them.
CREATE TABLE vendors (
    id            text PRIMARY KEY,
    display_name  text NOT NULL,
    -- Where the weekly scan starts for a vendor with no page there yet.
    sites         text[] NOT NULL DEFAULT '{}',
    currency      text NOT NULL DEFAULT 'USD',
    -- A vendor switched off is not read, and not published.
    enabled       boolean NOT NULL DEFAULT true,
    -- An administrator's word that its pages list all of its models.
    complete      boolean NOT NULL DEFAULT false,
    -- Whether every active page of the kind `page` was read without failure at the last check.
    -- The catalogue says `complete` only then: a consumer takes a model missing from a complete
    -- vendor as gone, and a page that could not be read says nothing about what it lists.
    read_complete boolean NOT NULL DEFAULT false,
    checked_at    timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

-- A page of a vendor's, or a machine-readable list that fills gaps.
CREATE TABLE pages (
    id          text PRIMARY KEY,
    vendor_id   text NOT NULL REFERENCES vendors (id) ON DELETE CASCADE,
    -- Normalised (`pacod-agg-jobs`, `pages.rs`).
    url         text NOT NULL,
    kind        text NOT NULL DEFAULT 'page' CHECK (kind IN ('page', 'feed')),
    -- For feeds: `pacod`, `litellm` or `openrouter`.
    format      text NOT NULL DEFAULT '',
    status      text NOT NULL DEFAULT 'active' CHECK (status IN ('proposed', 'active', 'dismissed')),
    -- `admin`, `scan` or `seed`.
    found_by    text NOT NULL DEFAULT 'admin',
    -- Where two pages of a vendor say different things about a model, the lower number wins.
    priority    integer NOT NULL DEFAULT 0,
    notes       text NOT NULL DEFAULT '',
    title       text NOT NULL DEFAULT '',
    score       double precision,
    moved_from  text,
    decided_by  text,
    decided_at  timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (vendor_id, url)
);

-- What the daily check finds out about a page.
CREATE TABLE page_state (
    page_id              text PRIMARY KEY REFERENCES pages (id) ON DELETE CASCADE,
    last_checked_at      timestamptz,
    -- When the page last answered and read as it should. A failed fetch leaves it, so that prices
    -- read from the page do not count as confirmed by a check that saw nothing.
    last_ok_at           timestamptz,
    -- When the page's content last differed from the reading before.
    last_changed_at      timestamptz,
    -- When the extraction model last read the page.
    last_read_at         timestamptz,
    -- Over the lines that carry models, prices and limits; a change here makes the page read.
    content_hash         text NOT NULL DEFAULT '',
    -- Over the whole text, for diagnosis.
    text_hash            text NOT NULL DEFAULT '',
    http_status          integer,
    -- Where the page ended up after redirects.
    final_url            text NOT NULL DEFAULT '',
    -- `read`, `unchanged`, `not_read` or `failed`.
    outcome              text NOT NULL DEFAULT '',
    message              text NOT NULL DEFAULT '',
    consecutive_failures integer NOT NULL DEFAULT 0,
    -- Set by the scan: `gone` (the page answers 404 or 410) or `moved` (it redirects elsewhere).
    flag                 text,
    moved_to             text,
    -- An administrator asked for this page to be checked, and read even when unchanged.
    check_requested      boolean NOT NULL DEFAULT false,
    force_read           boolean NOT NULL DEFAULT false,
    -- What the last successful reading found: an unchanged page settles from it.
    extraction           jsonb NOT NULL DEFAULT '{}'::jsonb,
    updated_at           timestamptz NOT NULL DEFAULT now()
);

-- Leases of the scraper's jobs. Who holds one, until when, and what was asked for.
CREATE TABLE jobs (
    name            text PRIMARY KEY,
    holder          text NOT NULL DEFAULT '',
    lease_until     timestamptz,
    -- When the job last started, for the schedule.
    last_started_at timestamptz,
    run_requested   boolean NOT NULL DEFAULT false,
    -- What a pending request names (pages, vendors); NULL asks for everything.
    run_scope       text[],
    current_run     text,
    -- When a scraper last asked for this job, granted or not, and what it said about itself: a
    -- poll that is not granted a lease writes nothing else, and the console must be able to tell
    -- that no scraper runs.
    last_polled_at  timestamptz,
    poll_status     jsonb
);

CREATE TABLE job_runs (
    id          text PRIMARY KEY,
    job         text NOT NULL,
    -- `schedule`, `manual`, `page`, `retry`.
    trigger     text NOT NULL,
    started_at  timestamptz NOT NULL,
    finished_at timestamptz,
    ok          boolean,
    message     text NOT NULL DEFAULT '',
    result      jsonb NOT NULL DEFAULT '{}'::jsonb,
    -- What the run covers, fixed when it starts: pages of a check, vendors of a scan.
    covers      text[] NOT NULL DEFAULT '{}'
);
CREATE INDEX job_runs_by_job ON job_runs (job, started_at DESC);

-- A model of a vendor, as the catalogue publishes it.
CREATE TABLE models (
    -- `agm_…`: opaque, one path segment, so that the console's data layer needs one parameter.
    id               text PRIMARY KEY,
    vendor_id        text NOT NULL REFERENCES vendors (id) ON DELETE CASCADE,
    -- The id the vendor's API takes, as it writes it.
    api_id           text NOT NULL,
    -- `api_id` in the form ids are compared in.
    key              text NOT NULL,
    aliases          text[] NOT NULL DEFAULT '{}',
    -- Whether a vendor's own page states `api_id`. A model known from a list alone is not
    -- proposed to gateways.
    api_id_confirmed boolean NOT NULL DEFAULT false,
    display_name     text,
    kind             text,
    status           text NOT NULL DEFAULT 'active',
    deprecated_at    timestamptz,
    sunset_at        timestamptz,
    successor        text,
    properties       jsonb NOT NULL DEFAULT '{}'::jsonb,
    parameters       jsonb NOT NULL DEFAULT '{}'::jsonb,
    -- Where each value came from, by field.
    provenance       jsonb NOT NULL DEFAULT '{}'::jsonb,
    first_seen_at    timestamptz NOT NULL DEFAULT now(),
    last_seen_at     timestamptz NOT NULL DEFAULT now(),
    -- Set when every page of a vendor was read and none named the model any more; cleared when
    -- one names it again. Such a model is not published.
    gone_at          timestamptz,
    UNIQUE (vendor_id, key)
);

-- Prices, append-only: a change adds a version and closes the one before.
CREATE TABLE price_versions (
    id             bigserial PRIMARY KEY,
    model_id       text NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    -- The catalogue's price, without its dates.
    price          jsonb NOT NULL,
    effective_from timestamptz NOT NULL DEFAULT now(),
    -- When its source last stated it.
    checked_at     timestamptz NOT NULL,
    superseded_at  timestamptz
);
CREATE INDEX price_versions_by_model ON price_versions (model_id, id DESC);
CREATE UNIQUE INDEX price_versions_current ON price_versions (model_id) WHERE superseded_at IS NULL;

-- A log of what changed in the catalogue, for the console.
CREATE TABLE changes (
    id        bigserial PRIMARY KEY,
    at        timestamptz NOT NULL DEFAULT now(),
    vendor_id text NOT NULL,
    model_id  text NOT NULL,
    -- `created`, `price`, `gone`, `back`, or a property's name.
    field     text NOT NULL,
    old       jsonb,
    new       jsonb,
    run_id    text
);
CREATE INDEX changes_by_time ON changes (at DESC, id DESC);
