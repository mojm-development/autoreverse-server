CREATE TABLE "artist_covers" (
	"artist" text PRIMARY KEY NOT NULL,
	"item_id" integer,
	"image_path" text,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "artist_cover_xor" CHECK (("artist_covers"."item_id" IS NULL) <> ("artist_covers"."image_path" IS NULL))
);
--> statement-breakpoint
ALTER TABLE "artist_covers" ADD CONSTRAINT "artist_covers_item_id_items_id_fk" FOREIGN KEY ("item_id") REFERENCES "public"."items"("id") ON DELETE cascade ON UPDATE no action;