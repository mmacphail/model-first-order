#!/usr/bin/env bash
#
# Register Avro schemas in Confluent Schema Registry and configure
# BACKWARD compatibility mode.
#
# Usage:
#   ./scripts/register-schemas.sh [SCHEMA_REGISTRY_URL]
#
# Defaults to http://localhost:8081 if no URL is provided.
set -euo pipefail

SCHEMA_REGISTRY_URL="${1:-http://localhost:8081}"
SCHEMAS_DIR="$(cd "$(dirname "$0")/../schemas" && pwd)"

SUBJECT="commerce.order.aggregate-value"
SCHEMA_FILE="$SCHEMAS_DIR/order-aggregate.avsc"

echo "==> Schema Registry: $SCHEMA_REGISTRY_URL"
echo "==> Subject:         $SUBJECT"
echo ""

# ── 1. Set compatibility mode to BACKWARD ────────────────────────────────────
echo "Setting compatibility mode to BACKWARD for subject '$SUBJECT'..."
compat_resp=$(curl -s -o /dev/null -w "%{http_code}" \
  -X PUT "$SCHEMA_REGISTRY_URL/config/$SUBJECT" \
  -H "Content-Type: application/vnd.schemaregistry.v1+json" \
  -d '{"compatibility": "BACKWARD"}')

# 404 is expected on first registration (subject doesn't exist yet) — we'll
# set it again after registering the schema.
if [ "$compat_resp" != "200" ] && [ "$compat_resp" != "404" ]; then
  echo "WARNING: Failed to set compatibility (HTTP $compat_resp), will retry after registration."
fi

# ── 2. Register the schema ──────────────────────────────────────────────────
echo "Registering schema from $SCHEMA_FILE..."

# Schema Registry expects the schema as a JSON-escaped string inside {"schema": "..."}.
schema_payload=$(jq -n --arg schema "$(cat "$SCHEMA_FILE")" '{"schemaType": "AVRO", "schema": $schema}')

register_resp=$(curl -s -X POST "$SCHEMA_REGISTRY_URL/subjects/$SUBJECT/versions" \
  -H "Content-Type: application/vnd.schemaregistry.v1+json" \
  -d "$schema_payload")

schema_id=$(echo "$register_resp" | jq -r '.id // empty')
if [ -z "$schema_id" ]; then
  echo "ERROR: Schema registration failed."
  echo "$register_resp" | jq .
  exit 1
fi
echo "Schema registered with id=$schema_id"

# ── 3. Ensure compatibility mode is set (retry after subject exists) ─────────
echo "Confirming BACKWARD compatibility on subject '$SUBJECT'..."
curl -s -X PUT "$SCHEMA_REGISTRY_URL/config/$SUBJECT" \
  -H "Content-Type: application/vnd.schemaregistry.v1+json" \
  -d '{"compatibility": "BACKWARD"}' | jq .

# ── 4. Verify ────────────────────────────────────────────────────────────────
echo ""
echo "==> Registered versions:"
curl -s "$SCHEMA_REGISTRY_URL/subjects/$SUBJECT/versions" | jq .

echo ""
echo "==> Compatibility mode:"
curl -s "$SCHEMA_REGISTRY_URL/config/$SUBJECT" | jq .

echo ""
echo "Done."
