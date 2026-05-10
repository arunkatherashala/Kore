# KORE Query Syntax Guide

## Version 0.4.0

### Table of Contents
1. [Basic SELECT](#basic-select)
2. [WHERE Clauses](#where-clauses)
3. [Joins](#joins)
4. [Window Functions](#window-functions)
5. [Subqueries](#subqueries)
6. [Aggregations](#aggregations)
7. [Advanced Examples](#advanced-examples)

---

## Basic SELECT

### Simple Column Selection
```sql
SELECT id, name, email FROM users;
```
Returns all rows from users table with specified columns.

### Select All Columns
```sql
SELECT * FROM users;
```
Returns all rows with all columns.

### Limit Results
```sql
SELECT * FROM users LIMIT 10;
```
Returns first 10 rows.

---

## WHERE Clauses

### Equality
```sql
SELECT * FROM users WHERE status = 1;
```
Returns rows where status equals 1.

### Comparison Operators
```sql
SELECT * FROM orders WHERE amount > 100;
SELECT * FROM products WHERE price <= 50;
SELECT * FROM transactions WHERE date < '2026-05-01';
```

### String Matching
```sql
SELECT * FROM users WHERE email = 'user@example.com';
```

---

## Joins

### INNER JOIN
```sql
SELECT u.id, u.name, o.order_id, o.amount
FROM users u
INNER JOIN orders o ON u.id = o.user_id;
```
Returns only matching rows from both tables.

### LEFT JOIN
```sql
SELECT u.id, u.name, o.order_id
FROM users u
LEFT JOIN orders o ON u.id = o.user_id;
```
Returns all users with matching orders (NULL if no order).

### RIGHT JOIN
```sql
SELECT u.id, o.order_id, o.amount
FROM users u
RIGHT JOIN orders o ON u.id = o.user_id;
```
Returns all orders with matching users (NULL if no user).

### Multiple Joins
```sql
SELECT u.name, o.order_id, p.product_name
FROM users u
INNER JOIN orders o ON u.id = o.user_id
INNER JOIN products p ON o.product_id = p.id;
```

---

## Window Functions

### Row Numbering
```sql
SELECT 
    name,
    salary,
    ROW_NUMBER() OVER (ORDER BY salary DESC) AS salary_rank
FROM employees;
```
Assigns sequential number ordered by salary (highest first).

### Ranking
```sql
SELECT 
    name,
    score,
    RANK() OVER (ORDER BY score DESC) AS score_rank
FROM competitors;
```
Assigns rank with gaps for ties.

### Dense Ranking
```sql
SELECT 
    name,
    department,
    salary,
    DENSE_RANK() OVER (PARTITION BY department ORDER BY salary DESC) AS dept_rank
FROM employees;
```
Assigns rank without gaps, partitioned by department.

### Lag & Lead
```sql
SELECT 
    date,
    sales,
    LAG(sales) OVER (ORDER BY date) AS prev_day_sales,
    LEAD(sales) OVER (ORDER BY date) AS next_day_sales
FROM daily_sales;
```
Access previous and next row values.

### Running Total
```sql
SELECT 
    date,
    amount,
    SUM(amount) OVER (ORDER BY date ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS running_total
FROM transactions;
```

### Partition-based Aggregation
```sql
SELECT 
    name,
    department,
    salary,
    AVG(salary) OVER (PARTITION BY department) AS dept_avg_salary
FROM employees;
```

---

## Subqueries

### Simple Subquery
```sql
SELECT * FROM (
    SELECT id, name FROM users WHERE status = 1
) AS active_users;
```

### Subquery with WHERE
```sql
SELECT * FROM users
WHERE id IN (
    SELECT user_id FROM orders WHERE amount > 1000
);
```
Returns users with orders exceeding $1000.

### Multiple Subqueries (CTE)
```sql
WITH high_value_orders AS (
    SELECT user_id, SUM(amount) as total
    FROM orders
    WHERE date > '2026-01-01'
    GROUP BY user_id
),
premium_users AS (
    SELECT * FROM users
    WHERE subscription_tier = 'premium'
)
SELECT p.name, h.total
FROM premium_users p
JOIN high_value_orders h ON p.id = h.user_id;
```

### Nested Subquery
```sql
SELECT * FROM users
WHERE id IN (
    SELECT user_id FROM orders
    WHERE product_id IN (
        SELECT id FROM products WHERE category = 'electronics'
    )
);
```

---

## Aggregations

### Count
```sql
SELECT COUNT(*) as total_users FROM users;
```
Returns number of rows.

### Sum
```sql
SELECT user_id, SUM(amount) as total_spent
FROM orders
GROUP BY user_id;
```

### Average
```sql
SELECT department, AVG(salary) as avg_salary
FROM employees
GROUP BY department;
```

### Min/Max
```sql
SELECT 
    MIN(price) as lowest_price,
    MAX(price) as highest_price
FROM products;
```

### Group By Multiple Columns
```sql
SELECT 
    date,
    category,
    COUNT(*) as order_count,
    SUM(amount) as total_amount
FROM orders
GROUP BY date, category;
```

---

## Advanced Examples

### Complete Analysis Query
```sql
WITH monthly_sales AS (
    SELECT 
        DATE_TRUNC('month', date) as month,
        product_id,
        SUM(amount) as monthly_total
    FROM orders
    WHERE date >= '2026-01-01'
    GROUP BY DATE_TRUNC('month', date), product_id
)
SELECT 
    p.name,
    m.month,
    m.monthly_total,
    ROW_NUMBER() OVER (PARTITION BY m.month ORDER BY m.monthly_total DESC) as month_rank,
    LAG(m.monthly_total) OVER (ORDER BY m.month) as prev_month_total
FROM monthly_sales m
JOIN products p ON m.product_id = p.id
WHERE m.monthly_total > 10000
ORDER BY m.month DESC, month_rank;
```

### Customer Segmentation
```sql
WITH customer_metrics AS (
    SELECT 
        u.id,
        u.name,
        COUNT(o.id) as order_count,
        SUM(o.amount) as lifetime_value,
        MAX(o.date) as last_order_date
    FROM users u
    LEFT JOIN orders o ON u.id = o.user_id
    GROUP BY u.id, u.name
)
SELECT 
    id,
    name,
    order_count,
    lifetime_value,
    CASE 
        WHEN lifetime_value > 10000 THEN 'VIP'
        WHEN lifetime_value > 1000 THEN 'Regular'
        ELSE 'Low-value'
    END as segment,
    RANK() OVER (ORDER BY lifetime_value DESC) as value_rank
FROM customer_metrics
WHERE order_count > 0;
```

### Time Series Analysis
```sql
SELECT 
    date,
    sales,
    SUM(sales) OVER (ORDER BY date ROWS BETWEEN 6 PRECEDING AND CURRENT ROW) as moving_avg_7d,
    LAG(sales) OVER (ORDER BY date) as prev_day,
    CASE 
        WHEN sales > LAG(sales) OVER (ORDER BY date) THEN 'UP'
        WHEN sales < LAG(sales) OVER (ORDER BY date) THEN 'DOWN'
        ELSE 'FLAT'
    END as trend
FROM daily_metrics
ORDER BY date DESC;
```

---

## Performance Tips

### 1. Use Indexes
```sql
-- Fast on indexed columns
SELECT * FROM users WHERE status = 1;

-- Slow on non-indexed columns
SELECT * FROM users WHERE email LIKE '%@example.com';
```

### 2. Filter Early
```sql
-- Good: Filter before join
SELECT u.name, o.amount
FROM users u
WHERE u.status = 1
INNER JOIN orders o ON u.id = o.user_id;

-- Less efficient: Filter after join
SELECT u.name, o.amount
FROM users u
INNER JOIN orders o ON u.id = o.user_id
WHERE u.status = 1;
```

### 3. Use Window Functions Instead of Joins
```sql
-- More efficient with window functions
SELECT 
    e.name,
    e.salary,
    AVG(e.salary) OVER (PARTITION BY e.department) as dept_avg
FROM employees e;
```

### 4. Limit Results
```sql
-- Always use LIMIT for large result sets
SELECT * FROM events LIMIT 1000;

-- Combine with ORDER BY for pagination
SELECT * FROM events ORDER BY date DESC LIMIT 100 OFFSET 50;
```

---

## Common Patterns

### Top N per Group
```sql
WITH ranked AS (
    SELECT 
        department,
        name,
        salary,
        ROW_NUMBER() OVER (PARTITION BY department ORDER BY salary DESC) as rank
    FROM employees
)
SELECT * FROM ranked WHERE rank <= 3;
```

### Running Difference
```sql
SELECT 
    date,
    sales,
    LAG(sales) OVER (ORDER BY date) as prev_sales,
    sales - LAG(sales) OVER (ORDER BY date) as sales_change
FROM daily_sales;
```

### Cohort Analysis
```sql
WITH cohorts AS (
    SELECT 
        u.id,
        DATE_TRUNC('month', u.created_at) as cohort_month,
        COUNT(o.id) as order_count
    FROM users u
    LEFT JOIN orders o ON u.id = o.user_id
    GROUP BY u.id, DATE_TRUNC('month', u.created_at)
)
SELECT 
    cohort_month,
    SUM(order_count) as total_orders,
    COUNT(*) as cohort_size
FROM cohorts
GROUP BY cohort_month;
```

---

## Need Help?

For more examples or to report issues:
- Check [API Documentation](API.md)
- Review [Architecture Guide](ARCHITECTURE.md)
- Visit [Troubleshooting Guide](TROUBLESHOOTING.md)
