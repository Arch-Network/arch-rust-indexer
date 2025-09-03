-- Local-only, idempotent conversion of TIMESTAMP to TIMESTAMPTZ (UTC semantics)
DO $$
BEGIN
    -- blocks.timestamp
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'blocks'
          AND column_name = 'timestamp'
          AND data_type = 'timestamp without time zone'
    ) THEN
        EXECUTE 'ALTER TABLE public.blocks ALTER COLUMN "timestamp" TYPE timestamptz USING "timestamp" AT TIME ZONE ''UTC'';';
    END IF;

    -- transactions.created_at
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'transactions'
          AND column_name = 'created_at'
          AND data_type = 'timestamp without time zone'
    ) THEN
        EXECUTE 'ALTER TABLE public.transactions ALTER COLUMN "created_at" TYPE timestamptz USING "created_at" AT TIME ZONE ''UTC'';';
    END IF;

    -- programs.first_seen_at / last_seen_at
    IF EXISTS (
        SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = 'programs'
    ) THEN
        IF EXISTS (
            SELECT 1
            FROM information_schema.columns
            WHERE table_schema = 'public'
              AND table_name = 'programs'
              AND column_name = 'first_seen_at'
              AND data_type = 'timestamp without time zone'
        ) THEN
            EXECUTE 'ALTER TABLE public.programs ALTER COLUMN "first_seen_at" TYPE timestamptz USING "first_seen_at" AT TIME ZONE ''UTC'';';
        END IF;

        IF EXISTS (
            SELECT 1
            FROM information_schema.columns
            WHERE table_schema = 'public'
              AND table_name = 'programs'
              AND column_name = 'last_seen_at'
              AND data_type = 'timestamp without time zone'
        ) THEN
            EXECUTE 'ALTER TABLE public.programs ALTER COLUMN "last_seen_at" TYPE timestamptz USING "last_seen_at" AT TIME ZONE ''UTC'';';
        END IF;
    END IF;

    -- mempool_transactions.added_at
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'mempool_transactions'
          AND column_name = 'added_at'
          AND data_type = 'timestamp without time zone'
    ) THEN
        EXECUTE 'ALTER TABLE public.mempool_transactions ALTER COLUMN "added_at" TYPE timestamptz USING "added_at" AT TIME ZONE ''UTC'';';
    END IF;
END
$$;


