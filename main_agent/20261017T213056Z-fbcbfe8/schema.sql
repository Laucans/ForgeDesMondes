--
-- PostgreSQL database dump
--


-- Dumped from database version 16.15 (Debian 16.15-1.pgdg13+2)
-- Dumped by pg_dump version 16.15 (Homebrew)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: ai_provider; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ai_provider (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    mode text NOT NULL,
    label text NOT NULL,
    url text,
    detected_api_type text,
    api_key text,
    "binary" text,
    generation_model text NOT NULL,
    summary_model text NOT NULL,
    last_test_ok boolean,
    last_test_at timestamp with time zone,
    last_test_message text,
    last_test_latency_ms integer,
    CONSTRAINT ai_provider_api_type_recognized CHECK (((detected_api_type IS NULL) OR (detected_api_type = ANY (ARRAY['cloud'::text, 'opencode'::text])))),
    CONSTRAINT ai_provider_mode_fields CHECK ((((mode = 'api'::text) AND (COALESCE(url, ''::text) <> ''::text) AND (COALESCE(detected_api_type, ''::text) <> ''::text) AND (COALESCE("binary", ''::text) = ''::text)) OR ((mode = 'binaire'::text) AND (COALESCE("binary", ''::text) = ANY (ARRAY['opencode'::text, 'claude-code'::text])) AND (COALESCE(url, ''::text) = ''::text))))
);


--
-- Name: ai_settings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ai_settings (
    id smallint DEFAULT 1 NOT NULL,
    active_provider_id uuid NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT ai_settings_single_row CHECK ((id = 1))
);


--
-- Name: ai_provider ai_provider_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_provider
    ADD CONSTRAINT ai_provider_pkey PRIMARY KEY (id);


--
-- Name: ai_settings ai_settings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_settings
    ADD CONSTRAINT ai_settings_pkey PRIMARY KEY (id);


--
-- Name: ai_settings ai_settings_active_provider_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_settings
    ADD CONSTRAINT ai_settings_active_provider_id_fkey FOREIGN KEY (active_provider_id) REFERENCES public.ai_provider(id) ON UPDATE CASCADE ON DELETE RESTRICT;


--
-- PostgreSQL database dump complete
--


