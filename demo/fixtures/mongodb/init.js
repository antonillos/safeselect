// Synthetic SafeSelect demo data. The demo uses `demo.sh reset` for a clean run.
const database = db.getSiblingDB('safeselect_demo');

['customers', 'products', 'orders', 'events'].forEach((name) => database[name].drop());

const segments = ['starter', 'growth', 'enterprise', 'nonprofit'];
const locales = ['en-GB', 'es-ES', 'ja-JP', 'fr-FR', 'de-DE', 'pt-BR'];
const cities = ['London', 'Madrid', 'Kyoto', 'Oslo', 'Toronto', 'Lisbon'];
const tags = ['security', 'api', 'priority', 'community', 'data', 'trial'];

const customers = Array.from({ length: 18 }, (_, index) => ({
  _id: `customer-${String(index + 1).padStart(3, '0')}`,
  displayName: `Demo Customer ${String.fromCharCode(65 + (index % 26))}-${index + 1}`,
  email: `customer-${index + 1}@example.test`,
  active: index % 5 !== 4,
  segment: segments[index % segments.length],
  profile: { locale: locales[index % locales.length], seats: (index + 1) * 3, newsletter: index % 2 === 0 },
  tags: [tags[index % tags.length], tags[(index + 2) % tags.length]],
  address: { city: cities[index % cities.length], postalCode: `TEST-${String(index + 1).padStart(3, '0')}` },
  createdAt: new Date(Date.UTC(2026, 0, 1 + index)),
}));
database.customers.insertMany(customers);
database.customers.createIndex({ segment: 1, active: 1 }, { name: 'segment_active' });

const categories = ['security', 'office', 'observability', 'database', 'connectivity', 'recovery'];
const products = Array.from({ length: 24 }, (_, index) => ({
  _id: `product-${String(index + 1).padStart(3, '0')}`,
  sku: `MONGO-${String(index + 1).padStart(3, '0')}`,
  name: `Demo Product ${index + 1}`,
  category: categories[index % categories.length],
  price: Number((12.5 + index * 8.75).toFixed(2)),
  available: index % 6 !== 5,
  attributes: { color: ['blue', 'green', 'amber', 'violet'][index % 4], weightGrams: 100 + index * 23, variants: [`v${index % 3 + 1}`, `v${index % 2 + 1}`] },
  dimensions: { width: index + 1, height: (index % 5) + 2, depth: (index % 3) + 1 },
}));
database.products.insertMany(products);
database.products.createIndex({ category: 1, available: 1 }, { name: 'category_available' });

const orders = Array.from({ length: 36 }, (_, index) => ({
  _id: `order-${String(index + 1).padStart(3, '0')}`,
  customerId: customers[index % customers.length]._id,
  status: ['paid', 'pending', 'cancelled', 'refunded'][index % 4],
  lines: [
    { sku: products[index % products.length].sku, quantity: (index % 3) + 1, price: products[index % products.length].price },
    { sku: products[(index + 7) % products.length].sku, quantity: 1, price: products[(index + 7) % products.length].price },
  ],
  totals: { subtotal: Number((50 + index * 13.25).toFixed(2)), taxRate: [0, 0.055, 0.1, 0.21][index % 4] },
  shipping: { city: cities[index % cities.length], addressLines: [`${index + 1} Demo Avenue`, 'Synthetic District'] },
  placedAt: new Date(Date.UTC(2026, 7, 1 + (index % 24), index % 23, 15)),
}));
database.orders.insertMany(orders);
database.orders.createIndex({ customerId: 1, placedAt: -1 }, { name: 'customer_recent_orders' });

database.events.insertMany(Array.from({ length: 48 }, (_, index) => ({
  _id: `event-${String(index + 1).padStart(3, '0')}`,
  orderId: orders[index % orders.length]._id,
  type: ['order.created', 'payment.captured', 'shipment.updated', 'customer.viewed'][index % 4],
  source: ['web', 'api', 'agent', 'import'][index % 4],
  payload: { attempt: (index % 3) + 1, labels: ['synthetic', `batch-${index % 4}`], ok: index % 7 !== 0 },
  occurredAt: new Date(Date.UTC(2026, 7, 1 + (index % 24), index % 23, 20)),
})));

print(`Seeded ${customers.length} customers, ${products.length} products, ${orders.length} orders, and 48 events`);
