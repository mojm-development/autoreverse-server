CREATE TABLE "library_config" (
	"id" integer PRIMARY KEY NOT NULL,
	"books_dir" text,
	"music_dir" text,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
