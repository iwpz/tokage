# Spec
## Goal
实现用户认证模块，包括注册、登录和 JWT 鉴权机制。

## Scope
- In scope: POST /api/register、POST /api/login，密码加盐哈希，JWT 颁发与验证
- Out of scope: 第三方 OAuth、短信验证码、多因素认证 (MFA)

## Decisions
- 加密方案：使用 Argon2id 替代 bcrypt，提供更强抗硬件破解能力
- 令牌存储：HttpOnly / Secure Cookie 存储 Refresh Token，内存持有 Access Token

# Contracts
## C1 认证 API
```ts
// 注册接口
POST /api/auth/register
Body: { username: string; email: string; password_hash: string }
Response: 201 { userId: string } | 400 | 409

// 登录接口
POST /api/auth/login
Body: { email: string; password_hash: string }
Response: 200 { accessToken: string; user: { id: string; email: string } } | 401
```

## C2 数据表模型
```sql
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL
);
```

# Tasks
```yaml
- id: T1
  domain: backend-api
  title: 用户数据库模型与迁移
  goal: 创建 users 表结构与 SQL 查询封装，实现 Argon2id 密码校验
  depends_on: []
  contracts: [C2]
  write_scope: ["src/db/users.rs", "migrations/V1__users.sql"]
  deliverables: ["SQL 迁移脚本", "User 模型与 CRUD 函数"]
  acceptance:
    commands: ["cargo test -p server db::users"]
    criteria: ["密码哈希符合 Argon2 规范且不可逆", "并发唯一索引防碰撞"]
  size: M
  difficulty: 2

- id: T2
  domain: backend-api
  title: 认证路由与 JWT 签发
  goal: 接入 C1 规范的注册与登录 HTTP 路由，返回有效 JWT
  depends_on: [T1]
  contracts: [C1, C2]
  write_scope: ["src/routes/auth.rs", "src/auth/jwt.rs"]
  deliverables: ["POST /register 处理器", "POST /login 处理器", "JWT 中间件"]
  acceptance:
    commands: ["cargo test -p server routes::auth"]
    criteria: ["成功与错误状态码与 C1 一致", "过期 Token 被中间件拒绝"]
  size: M
  difficulty: 2

- id: T3
  domain: testing
  title: 认证端到端集成测试
  goal: 覆盖注册、登录、Token 刷新与越权请求全链路
  depends_on: [T2]
  contracts: [C1]
  write_scope: ["tests/auth_e2e.rs"]
  deliverables: ["端到端自动化测试套件"]
  acceptance:
    commands: ["cargo test --test auth_e2e"]
    criteria: ["所有用例执行通过且无内存泄漏"]
  size: S
  difficulty: 1
```

# Notes
请注意数据库连接池的最大连接数，避免在并发测试中占满资源。
