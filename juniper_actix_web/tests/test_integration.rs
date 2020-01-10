use actix_web::{
    guard,
    test::{self, start, TestServer},
    web, App,
};
use futures::{
    executor::block_on,
    future::{ready, FutureExt},
};
use juniper::{
    http::tests::{run_http_test_suite, HTTPIntegration, TestResponse},
    tests::{model::Database, schema::Query},
    EmptyMutation, RootNode,
};
use juniper_actix_web::GraphQLRequest;
use std::sync::Arc;

type Schema = RootNode<'static, Query, EmptyMutation<Database>>;

struct Data {
    schema: Schema,
    context: Database,
}

struct TestActixWebIntegration {
    srv: TestServer,
    // sys: std::cell::RefCell<actix_rt::SystemRunner>,
}

impl TestActixWebIntegration {
    fn new(srv: TestServer) -> TestActixWebIntegration {
        // let sys = std::cell::RefCell::new(actix_rt::System::new("test-client"));
        TestActixWebIntegration { srv }
    }

    fn sys() -> actix_rt::SystemRunner {
        actix_rt::System::new("test-client")
    }

    fn test_endpoint(&self, endpoint: &'static str, expected: &str) {
        println!("URL: {}", endpoint);
        let request = self.srv.get(endpoint);
        println!("YEET");
        let body = actix_rt::System::new("test_endpoint").block_on(
            request
                .send()
                .then(move |response| response.expect(&format!("failed GET {}", endpoint)).body())
                .map(|body| String::from_utf8(body.unwrap().to_vec()).unwrap()),
        );
        println!("Claw");

        assert_eq!(body, expected);
    }
}

impl HTTPIntegration for TestActixWebIntegration {
    fn get(&self, url: &str) -> TestResponse {
        let request = self.srv.get(url);
        let response =
            actix_rt::System::new("get_request").block_on(request.send().then(make_test_response));
        response.expect(&format!("failed GET {}", url))
    }

    fn post(&self, url: &str, body: &str) -> TestResponse {
        let request = self
            .srv
            .post(url)
            .header(awc::http::header::CONTENT_TYPE, "application/json");
        actix_rt::System::new("post_request")
            .block_on(request.send_body(body.to_string()).then(make_test_response))
            .expect(&format!("failed POST {}", url))
    }
}

type Resp = Result<
    awc::ClientResponse<
        actix_http::encoding::Decoder<actix_http::Payload<actix_http::PayloadStream>>,
    >,
    awc::error::SendRequestError,
>;

async fn make_test_response(response: Resp) -> Result<TestResponse, awc::error::SendRequestError> {
    let mut response = response?;
    let status_code = response.status().as_u16() as i32;
    let body = response
        .body()
        .await
        .ok()
        .and_then(|body| String::from_utf8(body.to_vec()).ok());
    let content_type_header = response
        .headers()
        .get(actix_web::http::header::CONTENT_TYPE);
    let content_type = content_type_header
        .and_then(|ct| ct.to_str().ok())
        .unwrap_or_default()
        .to_string();

    Ok(TestResponse {
        status_code,
        body,
        content_type,
    })
}

// #[actix_rt::test]
#[test]
fn test_actix_web_integration() {
    let rt = actix_rt::Runtime::new().unwrap();
    println!("thing!");
    let schema = Schema::new(Query, EmptyMutation::<Database>::new());
    let context = Database::new();
    let data = Arc::new(Data { schema, context });

    let srv = start(move || {
        App::new().data(data.clone()).service(
            web::resource("/")
                .guard(guard::Any(guard::Get()).or(guard::Post()))
                .to(|st: web::Data<Arc<Data>>, data: GraphQLRequest| {
                    ready(data.execute(&st.schema, &st.context))
                }),
        )
    });

    println!("Srv created");

    let sync_test = TestActixWebIntegration::new(srv);
    run_http_test_suite(&sync_test);

    // #[cfg(feature = "async")]
    // {
    //     let mut srv = start(|| {
    //         App::new().service(web::resource("/").to(
    //             |st: web::Data<Arc<Data>>, data: GraphQLRequest| {
    //                 data.execute_async(&st.schema, &st.context)
    //             },
    //         ))
    //     });
    //     let async_test = TestActixWebIntegration::new(srv);
    //     run_http_test_suite(&async_test);
    // }

    // let mut srv = start(move || {
    //     App::new()
    //         .service(
    //             web::resource("/graphiql")
    //                 .route(web::get().to(move || graphiql_source(protocol_base_url))),
    //         )
    //         .service(
    //             web::resource("/playground")
    //                 .route(web::get().to(move || playground_source(protocol_base_url))),
    //         )
    // });

    // let tester = TestActixWebIntegration::new(srv);

    // tester.test_endpoint("graphiql", &graphiql::graphiql_source(protocol_base_url));

    // tester.test_endpoint(
    //     "playground",
    //     &playground::playground_source(protocol_base_url),
    // );
}
