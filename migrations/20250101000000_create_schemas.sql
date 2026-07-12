-- Create schemas for each bounded context.
-- Schemas provide logical isolation between domain modules.

CREATE SCHEMA IF NOT EXISTS identity;
CREATE SCHEMA IF NOT EXISTS activity_catalog;
CREATE SCHEMA IF NOT EXISTS recorded_activity;
CREATE SCHEMA IF NOT EXISTS route_editing;
CREATE SCHEMA IF NOT EXISTS route_versioning;
CREATE SCHEMA IF NOT EXISTS imports;
CREATE SCHEMA IF NOT EXISTS exports;
CREATE SCHEMA IF NOT EXISTS polar_integration;
CREATE SCHEMA IF NOT EXISTS platform;
