-- Add statii table for status records
CREATE TABLE IF NOT EXISTS statii (
    uri TEXT PRIMARY KEY,
    did TEXT NOT NULL,
    rkey TEXT NOT NULL,
    cid TEXT NOT NULL,
    record JSONB NOT NULL,
    indexed_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Add index for efficient status lookups
CREATE INDEX IF NOT EXISTS idx_statii_did_rkey ON statii (did, rkey);

-- Add missing columns to existing tables if they don't exist
DO $$ 
BEGIN
    -- Add origin_url to plays table if it doesn't exist
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'plays' AND column_name = 'origin_url') THEN
        ALTER TABLE plays ADD COLUMN origin_url TEXT;
    END IF;
    
    -- Add processed_time default if it doesn't have one
    BEGIN
        ALTER TABLE plays ALTER COLUMN processed_time SET DEFAULT NOW();
    EXCEPTION
        WHEN OTHERS THEN NULL; -- Ignore if already has default
    END;
    
    -- Ensure processed_time column exists
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns 
                   WHERE table_name = 'plays' AND column_name = 'processed_time') THEN
        ALTER TABLE plays ADD COLUMN processed_time TIMESTAMP WITH TIME ZONE DEFAULT NOW();
    END IF;
END $$;

-- Add profiles table if it doesn't exist
CREATE TABLE IF NOT EXISTS profiles (
    did TEXT PRIMARY KEY,
    handle TEXT,
    display_name TEXT,
    description TEXT,
    description_facets JSONB,
    avatar TEXT,
    banner TEXT,
    created_at TIMESTAMP WITH TIME ZONE
);

-- Add featured_items table if it doesn't exist
CREATE TABLE IF NOT EXISTS featured_items (
    did TEXT PRIMARY KEY,
    mbid TEXT NOT NULL,
    type TEXT NOT NULL
);