-- Nutrition and grocery records are portable business data. Search candidates
-- are short-lived local authorization tokens and are deliberately not synced.
CREATE TABLE nutrition_preferences (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    daily_target_kcal INTEGER CHECK (daily_target_kcal IS NULL OR daily_target_kcal BETWEEN 1 AND 100000),
    updated_at TEXT NOT NULL
);

CREATE TABLE recipes (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 120),
    servings REAL NOT NULL CHECK (servings > 0 AND servings <= 1000),
    instructions TEXT NOT NULL DEFAULT '' CHECK (length(instructions) <= 10000),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE recipe_ingredients (
    id TEXT PRIMARY KEY,
    recipe_id TEXT NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
    food_name TEXT NOT NULL CHECK (length(trim(food_name)) BETWEEN 1 AND 120),
    amount_grams REAL NOT NULL CHECK (amount_grams > 0 AND amount_grams <= 100000),
    kcal_per_100g REAL NOT NULL CHECK (kcal_per_100g BETWEEN 0 AND 900),
    source_title TEXT NOT NULL CHECK (length(trim(source_title)) BETWEEN 1 AND 240),
    source_url TEXT NOT NULL CHECK (source_url GLOB 'https://*'),
    checked_at TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX recipe_ingredients_recipe_order_idx ON recipe_ingredients(recipe_id, sort_order, id);

CREATE TABLE nutrition_entries (
    id TEXT PRIMARY KEY,
    log_date TEXT NOT NULL CHECK (length(log_date) = 10),
    meal_type TEXT NOT NULL CHECK (meal_type IN ('breakfast','lunch','dinner','snack','training')),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    amount REAL NOT NULL CHECK (amount > 0 AND amount <= 100000),
    unit TEXT NOT NULL CHECK (unit IN ('g','份','kcal')),
    kcal_per_unit REAL NOT NULL CHECK (kcal_per_unit BETWEEN 0 AND 900000),
    calories INTEGER NOT NULL CHECK (calories BETWEEN 0 AND 900000),
    state TEXT NOT NULL CHECK (state IN ('planned','eaten')),
    source_title TEXT,
    source_url TEXT CHECK (source_url IS NULL OR source_url GLOB 'https://*'),
    recipe_id TEXT REFERENCES recipes(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK ((unit = 'g' AND meal_type <> 'training') OR (unit = '份' AND meal_type <> 'training') OR (unit = 'kcal' AND meal_type = 'training')),
    CHECK (meal_type <> 'training' OR state = 'eaten')
);
CREATE INDEX nutrition_entries_date_idx ON nutrition_entries(log_date, meal_type, state, id);

CREATE TABLE shopping_items (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 120),
    quantity REAL NOT NULL CHECK (quantity > 0 AND quantity <= 100000),
    unit TEXT NOT NULL CHECK (length(trim(unit)) BETWEEN 1 AND 24),
    category TEXT NOT NULL DEFAULT '其他' CHECK (length(category) <= 40),
    notes TEXT NOT NULL DEFAULT '' CHECK (length(notes) <= 500),
    completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0,1)),
    source TEXT NOT NULL CHECK (source IN ('manual','import','recipe')),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX shopping_items_order_idx ON shopping_items(completed, sort_order, created_at, id);

CREATE TABLE nutrition_lookup_candidates (
    id TEXT PRIMARY KEY,
    candidate_json TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
CREATE INDEX nutrition_lookup_candidates_expiry_idx ON nutrition_lookup_candidates(expires_at);
