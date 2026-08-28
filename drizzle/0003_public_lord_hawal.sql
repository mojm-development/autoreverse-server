ALTER TABLE "items" ADD COLUMN "keep_episodes" integer;--> statement-breakpoint
ALTER TABLE "library_config" ADD COLUMN "podcast_keep_episodes" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "library_config" ADD CONSTRAINT "keep_episodes_range" CHECK ("library_config"."podcast_keep_episodes" BETWEEN 0 AND 50);