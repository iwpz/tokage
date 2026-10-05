# 设计目标与规范

## 目标
快速修复登录会话过期的死锁问题。

## 范围
- 包含：修复 mutex 锁争用
- 不包含：重构整个认证架构

## 核心决策
- 使用无锁读缓存替代全局 RwLock。

# 契约与规范
## C1 会话缓存
```rust
pub struct SessionCache {
    sessions: dashmap::DashMap<String, Session>,
}
```

# 任务分解
```yaml
tasks:
	- id: 
	  title: "死锁消除"
	  domain: ""
	  write_scope: "src/session.rs"
	  size: small
	  acceptance:
	    criteria: ["高并发测试无死锁超时"]
	- id: 
	  title: "压力测试用例"
	  domain: "testing"
	  depends_on: ["T1"]
	  contracts: ["C1"]
	  write_scope: ["tests/concurrency.rs"]
	  difficulty: 99
```

# 注意事项
修复必须在主线程阻塞前释放写锁。
