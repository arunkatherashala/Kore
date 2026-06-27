//! Access control and permissions management

use crate::error::{Result, SecurityError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

/// Permission type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Permission {
    /// Read permission
    Read,
    /// Write permission
    Write,
    /// Delete permission
    Delete,
    /// Execute permission
    Execute,
    /// Admin permission
    Admin,
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Permission::Read => write!(f, "READ"),
            Permission::Write => write!(f, "WRITE"),
            Permission::Delete => write!(f, "DELETE"),
            Permission::Execute => write!(f, "EXECUTE"),
            Permission::Admin => write!(f, "ADMIN"),
        }
    }
}

/// Role definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    /// Role name
    pub name: String,
    /// Role permissions
    pub permissions: HashSet<Permission>,
    /// Role description
    pub description: String,
}

impl Role {
    /// Create new role
    pub fn new(name: String, description: String) -> Self {
        Role {
            name,
            permissions: HashSet::new(),
            description,
        }
    }

    /// Add permission to role
    pub fn add_permission(&mut self, permission: Permission) {
        self.permissions.insert(permission);
    }

    /// Check if role has permission
    pub fn has_permission(&self, permission: Permission) -> bool {
        if self.permissions.contains(&Permission::Admin) {
            return true;
        }
        self.permissions.contains(&permission)
    }
}

/// Subject (user/service)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    /// Subject ID
    pub id: String,
    /// Subject name
    pub name: String,
    /// Assigned roles
    pub roles: HashSet<String>,
}

impl Subject {
    /// Create new subject
    pub fn new(id: String, name: String) -> Self {
        Subject {
            id,
            name,
            roles: HashSet::new(),
        }
    }

    /// Add role to subject
    pub fn add_role(&mut self, role: String) {
        self.roles.insert(role);
    }

    /// Remove role from subject
    pub fn remove_role(&mut self, role: &str) {
        self.roles.remove(role);
    }
}

/// Protected resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// Resource ID
    pub id: String,
    /// Resource type
    pub resource_type: String,
    /// Owner (subject ID)
    pub owner: String,
    /// ACL: subject -> permissions
    pub acl: std::collections::HashMap<String, HashSet<Permission>>,
}

impl Resource {
    /// Create new resource
    pub fn new(id: String, resource_type: String, owner: String) -> Self {
        Resource {
            id,
            resource_type,
            owner,
            acl: std::collections::HashMap::new(),
        }
    }

    /// Grant permission to subject
    pub fn grant_permission(&mut self, subject_id: String, permission: Permission) {
        self.acl
            .entry(subject_id)
            .or_insert_with(HashSet::new)
            .insert(permission);
    }

    /// Revoke permission from subject
    pub fn revoke_permission(&mut self, subject_id: &str, permission: Permission) {
        if let Some(perms) = self.acl.get_mut(subject_id) {
            perms.remove(&permission);
        }
    }

    /// Check if subject has permission
    pub fn has_permission(&self, subject_id: &str, permission: Permission) -> bool {
        if self.owner == subject_id {
            return true; // Owner has all permissions
        }
        self.acl
            .get(subject_id)
            .map(|perms| perms.contains(&permission))
            .unwrap_or(false)
    }
}

/// Access control system
#[async_trait]
pub trait AccessControl: Send + Sync {
    /// Check if subject has permission on resource
    async fn check_permission(
        &self,
        subject_id: &str,
        resource_id: &str,
        permission: Permission,
    ) -> Result<bool>;

    /// Create role
    async fn create_role(&self, role: Role) -> Result<()>;

    /// Delete role
    async fn delete_role(&self, role_name: &str) -> Result<()>;

    /// Assign role to subject
    async fn assign_role(&self, subject_id: &str, role_name: &str) -> Result<()>;

    /// Revoke role from subject
    async fn revoke_role(&self, subject_id: &str, role_name: &str) -> Result<()>;

    /// Grant permission on resource
    async fn grant_permission(
        &self,
        subject_id: &str,
        resource_id: &str,
        permission: Permission,
    ) -> Result<()>;

    /// Revoke permission on resource
    async fn revoke_permission(
        &self,
        subject_id: &str,
        resource_id: &str,
        permission: Permission,
    ) -> Result<()>;
}

/// In-memory access control implementation
pub struct InMemoryAccessControl {
    roles: Arc<parking_lot::Mutex<std::collections::HashMap<String, Role>>>,
    subjects: Arc<parking_lot::Mutex<std::collections::HashMap<String, Subject>>>,
    resources: Arc<parking_lot::Mutex<std::collections::HashMap<String, Resource>>>,
}

impl InMemoryAccessControl {
    /// Create new access control
    pub fn new() -> Self {
        InMemoryAccessControl {
            roles: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
            subjects: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
            resources: Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Get role count
    pub fn role_count(&self) -> usize {
        self.roles.lock().len()
    }
}

impl Default for InMemoryAccessControl {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AccessControl for InMemoryAccessControl {
    async fn check_permission(
        &self,
        subject_id: &str,
        resource_id: &str,
        permission: Permission,
    ) -> Result<bool> {
        let resources = self.resources.lock();
        let resource = resources.get(resource_id).ok_or_else(|| {
            SecurityError::InvalidPermission("Resource not found".to_string())
        })?;

        Ok(resource.has_permission(subject_id, permission))
    }

    async fn create_role(&self, role: Role) -> Result<()> {
        self.roles.lock().insert(role.name.clone(), role);
        Ok(())
    }

    async fn delete_role(&self, role_name: &str) -> Result<()> {
        self.roles.lock().remove(role_name);
        Ok(())
    }

    async fn assign_role(&self, subject_id: &str, role_name: &str) -> Result<()> {
        let roles = self.roles.lock();
        if !roles.contains_key(role_name) {
            return Err(SecurityError::InvalidPermission("Role not found".to_string()));
        }

        drop(roles);

        let mut subjects = self.subjects.lock();
        subjects
            .entry(subject_id.to_string())
            .or_insert_with(|| Subject::new(subject_id.to_string(), subject_id.to_string()))
            .add_role(role_name.to_string());

        Ok(())
    }

    async fn revoke_role(&self, subject_id: &str, role_name: &str) -> Result<()> {
        let mut subjects = self.subjects.lock();
        if let Some(subject) = subjects.get_mut(subject_id) {
            subject.remove_role(role_name);
        }
        Ok(())
    }

    async fn grant_permission(
        &self,
        subject_id: &str,
        resource_id: &str,
        permission: Permission,
    ) -> Result<()> {
        let mut resources = self.resources.lock();
        resources
            .entry(resource_id.to_string())
            .or_insert_with(|| {
                Resource::new(
                    resource_id.to_string(),
                    "generic".to_string(),
                    "system".to_string(),
                )
            })
            .grant_permission(subject_id.to_string(), permission);

        Ok(())
    }

    async fn revoke_permission(
        &self,
        subject_id: &str,
        resource_id: &str,
        permission: Permission,
    ) -> Result<()> {
        let mut resources = self.resources.lock();
        if let Some(resource) = resources.get_mut(resource_id) {
            resource.revoke_permission(subject_id, permission);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_creation() {
        let mut role = Role::new("admin".to_string(), "Administrator".to_string());
        role.add_permission(Permission::Admin);

        assert!(role.has_permission(Permission::Read));
        assert!(role.has_permission(Permission::Write));
    }

    #[test]
    fn test_subject_roles() {
        let mut subject = Subject::new("user1".to_string(), "User 1".to_string());
        subject.add_role("admin".to_string());

        assert!(subject.roles.contains("admin"));

        subject.remove_role("admin");
        assert!(!subject.roles.contains("admin"));
    }

    #[test]
    fn test_resource_acl() {
        let mut resource = Resource::new("file1".to_string(), "file".to_string(), "user1".to_string());

        assert!(resource.has_permission("user1", Permission::Read)); // Owner

        resource.grant_permission("user2".to_string(), Permission::Read);
        assert!(resource.has_permission("user2", Permission::Read));
    }

    #[tokio::test]
    async fn test_access_control() {
        let ac = InMemoryAccessControl::new();

        let mut role = Role::new("reader".to_string(), "Reader".to_string());
        role.add_permission(Permission::Read);
        ac.create_role(role).await.unwrap();

        assert_eq!(ac.role_count(), 1);
    }
}
