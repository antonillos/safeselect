-- Synthetic SafeSelect demo data. Recreated by `demo.sh reset`.
CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE demo_customers (
    customer_id UUID PRIMARY KEY,
    display_name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE,
    segment TEXT NOT NULL CHECK (segment IN ('starter', 'growth', 'enterprise', 'nonprofit')),
    active BOOLEAN NOT NULL,
    preferences JSONB NOT NULL,
    tags TEXT[] NOT NULL,
    credit_limit NUMERIC(12, 2) NOT NULL,
    last_seen_at TIMESTAMPTZ,
    last_seen_ip INET,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE demo_products (
    product_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    sku TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    unit_price NUMERIC(10, 2) NOT NULL,
    available BOOLEAN NOT NULL,
    attributes JSONB NOT NULL,
    dimensions DOUBLE PRECISION[] NOT NULL,
    released_on DATE NOT NULL
);

CREATE TABLE demo_orders (
    order_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    customer_id UUID NOT NULL REFERENCES demo_customers(customer_id),
    status TEXT NOT NULL CHECK (status IN ('paid', 'pending', 'cancelled', 'refunded')),
    subtotal NUMERIC(12, 2) NOT NULL,
    tax_rate NUMERIC(4, 3) NOT NULL,
    placed_at TIMESTAMPTZ NOT NULL,
    delivery_window TSTZRANGE,
    shipping_address JSONB NOT NULL
);

CREATE TABLE demo_order_items (
    order_id BIGINT NOT NULL REFERENCES demo_orders(order_id) ON DELETE CASCADE,
    product_id BIGINT NOT NULL REFERENCES demo_products(product_id),
    quantity SMALLINT NOT NULL CHECK (quantity > 0),
    unit_price NUMERIC(10, 2) NOT NULL,
    PRIMARY KEY (order_id, product_id)
);

CREATE TABLE demo_events (
    event_id UUID PRIMARY KEY,
    order_id BIGINT REFERENCES demo_orders(order_id),
    event_type TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    duration INTERVAL,
    source_ip INET,
    payload JSONB NOT NULL,
    checksum BYTEA NOT NULL
);

CREATE INDEX demo_customers_segment_idx ON demo_customers (segment);
CREATE INDEX demo_products_category_available_idx ON demo_products (category, available);
CREATE INDEX demo_orders_placed_at_idx ON demo_orders (placed_at DESC);
CREATE INDEX demo_events_type_time_idx ON demo_events (event_type, occurred_at DESC);

INSERT INTO demo_customers VALUES
('10000000-0000-4000-8000-000000000001', 'Aster Labs', 'aster@example.test', 'enterprise', true, '{"locale":"en-GB","newsletter":true,"seats":48}', ARRAY['priority','security'], 25000.00, '2026-08-24 09:10+00', '192.0.2.11', '2026-01-04 08:00+00'),
('10000000-0000-4000-8000-000000000002', 'Blue Dune Studio', 'blue-dune@example.test', 'growth', true, '{"locale":"en-US","newsletter":false,"seats":12}', ARRAY['design'], 8000.00, '2026-08-23 14:25+00', '198.51.100.12', '2026-01-08 11:30+00'),
('10000000-0000-4000-8000-000000000003', 'Cedar Works', 'cedar@example.test', 'starter', true, '{"locale":"en-CA","newsletter":true,"seats":3}', ARRAY['trial','api'], 1200.00, '2026-08-22 18:40+00', '203.0.113.13', '2026-02-14 16:10+00'),
('10000000-0000-4000-8000-000000000004', 'Delta Orchard', 'delta@example.test', 'nonprofit', true, '{"locale":"fr-FR","newsletter":true,"seats":20}', ARRAY['discount','community'], 5000.00, '2026-08-21 07:05+00', '192.0.2.14', '2026-02-21 09:45+00'),
('10000000-0000-4000-8000-000000000005', 'Ember Transit', 'ember@example.test', 'enterprise', false, '{"locale":"de-DE","newsletter":false,"seats":120}', ARRAY['paused','security'], 50000.00, NULL, NULL, '2026-03-02 12:00+00'),
('10000000-0000-4000-8000-000000000006', 'Fjord Analytics', 'fjord@example.test', 'growth', true, '{"locale":"nb-NO","newsletter":false,"seats":18}', ARRAY['data','api'], 11000.00, '2026-08-20 20:15+00', '198.51.100.16', '2026-03-16 10:20+00'),
('10000000-0000-4000-8000-000000000007', 'Glass Garden', 'glass-garden@example.test', 'starter', true, '{"locale":"es-ES","newsletter":true,"seats":5}', ARRAY['trial'], 1800.00, '2026-08-19 13:55+00', '203.0.113.17', '2026-04-01 15:00+00'),
('10000000-0000-4000-8000-000000000008', 'Harbor Signal', 'harbor@example.test', 'enterprise', true, '{"locale":"ja-JP","newsletter":true,"seats":64}', ARRAY['priority','iot'], 30000.00, '2026-08-18 05:30+00', '192.0.2.18', '2026-04-12 06:40+00'),
('10000000-0000-4000-8000-000000000009', 'Indigo Current', 'indigo@example.test', 'growth', false, '{"locale":"pt-BR","newsletter":false,"seats":9}', ARRAY['paused'], 6000.00, NULL, NULL, '2026-05-03 19:25+00'),
('10000000-0000-4000-8000-00000000000a', 'Juniper North', 'juniper@example.test', 'starter', true, '{"locale":"nl-NL","newsletter":true,"seats":2}', ARRAY['api','early-access'], 900.00, '2026-08-17 11:11+00', '198.51.100.20', '2026-05-20 08:15+00'),
('10000000-0000-4000-8000-00000000000b', 'Kite Assembly', 'kite@example.test', 'nonprofit', true, '{"locale":"it-IT","newsletter":false,"seats":30}', ARRAY['community','discount'], 7000.00, '2026-08-16 16:45+00', '203.0.113.21', '2026-06-02 13:50+00'),
('10000000-0000-4000-8000-00000000000c', 'Lumen Field', 'lumen@example.test', 'growth', true, '{"locale":"sv-SE","newsletter":true,"seats":15}', ARRAY['security','data'], 9500.00, '2026-08-15 22:20+00', '192.0.2.22', '2026-06-18 17:35+00');

INSERT INTO demo_products (sku, name, category, unit_price, available, attributes, dimensions, released_on)
SELECT format('DEMO-%s', lpad(i::text, 3, '0')),
       (ARRAY['Read-only notebook','Audit marker','Policy compass','Schema lens','Query lantern','Safe connector','Fixture atlas','Index guide','Data prism','Boundary badge','MCP cable','Recovery card'])[i],
       (ARRAY['security','office','security','observability','office','connectivity','documentation','database','observability','security','connectivity','recovery'])[i],
       round((9.95 + i * 17.35)::numeric, 2),
       i % 5 <> 0,
       jsonb_build_object('color', (ARRAY['blue','green','amber','violet'])[((i - 1) % 4) + 1], 'weight_grams', i * 37, 'warranty_months', (i % 3 + 1) * 12),
       ARRAY[i::double precision, (i + 3)::double precision, (i % 5 + 1)::double precision],
       date '2025-01-01' + (i * 31)
FROM generate_series(1, 12) AS s(i);

INSERT INTO demo_orders (customer_id, status, subtotal, tax_rate, placed_at, delivery_window, shipping_address)
SELECT c.customer_id,
       (ARRAY['paid','pending','cancelled','refunded'])[((i - 1) % 4) + 1],
       round((45 + i * 23.75)::numeric, 2),
       (ARRAY[0.000, 0.055, 0.100, 0.210])[(i % 4) + 1],
       timestamptz '2026-08-01 08:00+00' + (i * interval '9 hours'),
       CASE WHEN i % 4 = 0 THEN NULL ELSE tstzrange(
           timestamptz '2026-08-03 09:00+00' + (i * interval '1 day'),
           timestamptz '2026-08-03 13:00+00' + (i * interval '1 day'), '[)') END,
       jsonb_build_object('city', (ARRAY['London','Madrid','Oslo','Kyoto','Toronto','Lisbon'])[((i - 1) % 6) + 1], 'postal_code', format('TEST-%s', lpad(i::text, 3, '0')), 'lines', jsonb_build_array(format('%s Demo Street', i)))
FROM generate_series(1, 24) AS s(i)
JOIN LATERAL (SELECT customer_id FROM demo_customers ORDER BY customer_id OFFSET ((i - 1) % 12) LIMIT 1) c ON true;

INSERT INTO demo_order_items
SELECT o.order_id, p.product_id, ((o.order_id + p.product_id) % 4 + 1)::smallint, p.unit_price
FROM demo_orders o
JOIN demo_products p ON p.product_id = ((o.order_id - 1) % 12) + 1;

INSERT INTO demo_events
SELECT gen_random_uuid(), o.order_id,
       (ARRAY['order.created','payment.captured','shipment.updated','customer.viewed'])[((o.order_id - 1) % 4) + 1],
       o.placed_at + interval '3 minutes',
       make_interval(secs => (o.order_id % 9)::int),
       ('203.0.113.' || ((o.order_id % 20) + 1))::inet,
       jsonb_build_object('source', (ARRAY['web','api','agent','import'])[((o.order_id - 1) % 4) + 1], 'attempt', (o.order_id % 3) + 1, 'labels', jsonb_build_array('synthetic', format('batch-%s', o.order_id % 4))),
       digest(format('demo-event-%s', o.order_id), 'sha256')
FROM demo_orders o;
