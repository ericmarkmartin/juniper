use juniper::{graphql_value, RootNode, Value};

#[derive(juniper::GraphQLEnum)]
enum UserKind {
    Admin,
    User,
    Guest,
}

struct User {
    id: u64,
    name: String,
    kind: UserKind,
}

#[juniper::graphql_object]
impl User {
    async fn name(&self) -> &str {
        &self.name
    }

    async fn friends(&self) -> Vec<User> {
        let friends = (0..10)
            .map(|index| User {
                id: index,
                name: format!("user{}", index),
                kind: UserKind::User,
            })
            .collect();
        friends
    }

    async fn kind(&self) -> &UserKind {
        &self.kind
    }

    async fn delayed() -> bool {
        let duration = std::time::Duration::from_millis(100);
        tokio::time::delay_for(duration).await;
        true
    }
}

struct Query;

#[juniper::graphql_object]
impl Query {
    fn field_sync(&self) -> &'static str {
        "field_sync"
    }

    async fn field_async_plain() -> String {
        "field_async_plain".to_string()
    }

    fn user(id: String) -> User {
        User {
            id: 1,
            name: id,
            kind: UserKind::User,
        }
    }

    async fn delayed() -> bool {
        let duration = std::time::Duration::from_millis(100);
        tokio::time::delay_for(duration).await;
        true
    }
}

struct Mutation;

#[juniper::graphql_object]
impl Mutation {}

fn run<O>(f: impl std::future::Future<Output = O>) -> O {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(f)
}

#[test]
fn async_simple() {
    let schema = RootNode::new(Query, Mutation);
    let doc = r#"
        query { 
            fieldSync
            fieldAsyncPlain 
            delayed  
            user(id: "user1") {
                kind
                name
                delayed
            }
        }
    "#;

    let vars = Default::default();
    let f = juniper::execute_async(doc, None, &schema, &vars, &());

    let (res, errs) = run(f).unwrap();

    assert!(errs.is_empty());

    let mut obj = res.into_object().unwrap();
    obj.sort_by_field();
    let value = Value::Object(obj);

    assert_eq!(
        value,
        graphql_value!({
            "delayed": true,
            "fieldAsyncPlain": "field_async_plain",
            "fieldSync": "field_sync",
            "user": {
                "delayed": true,
                "kind": "USER",
                "name": "user1",
            },
        }),
    );
}

fn main() {}
