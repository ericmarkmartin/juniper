/*!

# juniper_actix_web

This repository contains the [Actix web][Actix web] web server integration for
[Juniper][Juniper], a [GraphQL][GraphQL] implementation for Rust.

## Documentation

For documentation, including guides and examples, check out [Juniper][Juniper].

A basic usage example can also be found in the [Api documentation][documentation].

## Examples

Check [examples/actix_web_server.rs][example] for example code of a working Rocket
server with GraphQL handlers.

## Links

* [Juniper][Juniper]
* [Api Reference][documentation]
* [Actix web][Actix web]

## License

This project is under the BSD-2 license.

Check the LICENSE file for details.

[Actix web]: https://actix.rs
[Juniper]: https://github.com/graphql-rust/juniper
[GraphQL]: http://graphql.org
[documentation]: https://docs.rs/juniper_actix_web
[example]: https://github.com/graphql-rust/juniper_actix_web/blob/master/examples/actix_web_server.rs

*/

// #[cfg(feature = "async")]
use futures::future::{err, ok, ready, Either, FutureExt, LocalBoxFuture, Ready};

#[cfg(feature = "async")]
use juniper::GraphQLTypeAsync;

use actix_web::{
    dev,
    http::{Method, StatusCode},
    web, Error, FromRequest, HttpRequest, HttpResponse, Responder, Result,
};

use juniper::{
    http::{
        graphiql, playground, GraphQLRequest as JuniperGraphQLRequest,
        GraphQLResponse as JuniperGraphQLResponse,
    },
    serde::Deserialize,
    DefaultScalarValue, FieldError, GraphQLType, InputValue, RootNode, ScalarValue,
};

use serde::{de, Deserializer};

fn deserialize_non_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let v = Vec::<T>::deserialize(deserializer)?;

    if v.is_empty() {
        Err(de::Error::invalid_length(0, &"a positive integer"))
    } else {
        Ok(v)
    }
}

#[derive(Debug, serde_derive::Deserialize, PartialEq)]
#[serde(untagged)]
#[serde(bound = "InputValue<S>: Deserialize<'de>")]
enum GraphQLBatchRequest<S = DefaultScalarValue>
where
    S: ScalarValue,
{
    Single(JuniperGraphQLRequest<S>),
    #[serde(deserialize_with = "deserialize_non_empty_vec")]
    Batch(Vec<JuniperGraphQLRequest<S>>),
}

#[derive(serde_derive::Serialize)]
#[serde(untagged)]
enum GraphQLBatchResponse<'a, S = DefaultScalarValue>
where
    S: ScalarValue,
{
    Single(JuniperGraphQLResponse<'a, S>),
    Batch(Vec<JuniperGraphQLResponse<'a, S>>),
}

impl<S> GraphQLBatchRequest<S>
where
    S: ScalarValue,
{
    pub fn execute<'a, CtxT, QueryT, MutationT>(
        &'a self,
        root_node: &'a RootNode<QueryT, MutationT, S>,
        context: &CtxT,
    ) -> GraphQLBatchResponse<'a, S>
    where
        QueryT: GraphQLType<S, Context = CtxT>,
        MutationT: GraphQLType<S, Context = CtxT>,
    {
        match self {
            &GraphQLBatchRequest::Single(ref request) => {
                GraphQLBatchResponse::Single(request.execute(root_node, context))
            }
            &GraphQLBatchRequest::Batch(ref requests) => GraphQLBatchResponse::Batch(
                requests
                    .iter()
                    .map(|request| request.execute(root_node, context))
                    .collect(),
            ),
        }
    }

    #[cfg(feature = "async")]
    pub async fn execute_async<'a, CtxT, QueryT, MutationT>(
        &'a self,
        root_node: &'a RootNode<'a, QueryT, MutationT, S>,
        context: &'a CtxT,
    ) -> GraphQLBatchResponse<'a, S>
    where
        QueryT: GraphQLTypeAsync<S, Context = CtxT> + Send + Sync,
        QueryT::TypeInfo: Send + Sync,
        MutationT: GraphQLTypeAsync<S, Context = CtxT> + Send + Sync,
        MutationT::TypeInfo: Send + Sync,
        CtxT: Send + Sync,
        S: Send + Sync,
    {
        match self {
            &GraphQLBatchRequest::Single(ref request) => {
                let res = request.execute_async(root_node, context).await;
                GraphQLBatchResponse::Single(res)
            }
            &GraphQLBatchRequest::Batch(ref requests) => {
                let futures = requests
                    .iter()
                    .map(|request| request.execute_async(root_node, context))
                    .collect::<Vec<_>>();
                let responses = futures::future::join_all(futures).await;
                GraphQLBatchResponse::Batch(responses)
            }
        }
    }

    pub fn operation_names(&self) -> Vec<Option<&str>> {
        match self {
            GraphQLBatchRequest::Single(request) => vec![request.operation_name()],
            GraphQLBatchRequest::Batch(requests) => {
                requests.iter().map(|req| req.operation_name()).collect()
            }
        }
    }
}

impl<'a, S> GraphQLBatchResponse<'a, S>
where
    S: ScalarValue,
{
    fn is_ok(&self) -> bool {
        match self {
            &GraphQLBatchResponse::Single(ref response) => response.is_ok(),
            &GraphQLBatchResponse::Batch(ref responses) => responses
                .iter()
                .fold(true, |ok, response| ok && response.is_ok()),
        }
    }
}

/// Single wrapper around an incoming GraphQL request
///
/// See the http module for information. This type can be constructed
/// automatically requests by implementing the FromRequest trait.
#[derive(Debug, PartialEq, serde_derive::Deserialize)]
pub struct GraphQLRequest<S = DefaultScalarValue>(GraphQLBatchRequest<S>)
where
    S: ScalarValue;

/// Simple wrapper around the result of executing a GraphQL query
pub struct GraphQLResponse(pub StatusCode, pub String);

/// Generate a HTML page containing GraphiQL
pub fn graphiql_source(graphql_endpoint_url: &str) -> HttpResponse {
    let html = graphiql::graphiql_source(graphql_endpoint_url);
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
}

/// Generate a HTML page containing GraphQL Playground
pub fn playground_source(graphql_endpoint_url: &str) -> HttpResponse {
    let html = playground::playground_source(graphql_endpoint_url);
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
}

impl<S> GraphQLRequest<S>
where
    S: ScalarValue,
{
    /// Execute an incoming GraphQL query
    pub fn execute<CtxT, QueryT, MutationT>(
        &self,
        root_node: &RootNode<QueryT, MutationT, S>,
        context: &CtxT,
    ) -> GraphQLResponse
    where
        QueryT: GraphQLType<S, Context = CtxT>,
        MutationT: GraphQLType<S, Context = CtxT>,
    {
        let response = self.0.execute(root_node, context);
        let status = if response.is_ok() {
            StatusCode::OK
        } else {
            StatusCode::BAD_REQUEST
        };
        let json = serde_json::to_string(&response).unwrap();

        GraphQLResponse(status, json)
    }

    #[cfg(feature = "async")]
    pub async fn execute_async<'a, CtxT, QueryT, MutationT>(
        &'a self,
        root_node: &'a RootNode<'a, QueryT, MutationT, S>,
        context: &'a CtxT,
    ) -> GraphQLResponse
    where
        QueryT: GraphQLTypeAsync<S, Context = CtxT> + Send + Sync,
        QueryT::TypeInfo: Send + Sync,
        MutationT: GraphQLTypeAsync<S, Context = CtxT> + Send + Sync,
        MutationT::TypeInfo: Send + Sync,
        CtxT: Send + Sync,
        S: Send + Sync,
    {
        let response = self.0.execute_async(root_node, context).await;
        let status = if response.is_ok() {
            StatusCode::OK
        } else {
            StatusCode::BAD_REQUEST
        };
        let json = serde_json::to_string(&response).unwrap();

        GraphQLResponse(status, json)
    }

    /// Returns the operation names associated with this request.
    ///
    /// For batch requests there will be multiple names.
    pub fn operation_names(&self) -> Vec<Option<&str>> {
        self.0.operation_names()
    }
}

impl GraphQLResponse {
    pub fn error(error: FieldError) -> Self {
        let response = JuniperGraphQLResponse::error(error);
        let json = serde_json::to_string(&response).unwrap();
        GraphQLResponse(StatusCode::BAD_REQUEST, json)
    }

    pub fn custom(status: StatusCode, response: serde_json::Value) -> Self {
        let json = serde_json::to_string(&response).unwrap();
        GraphQLResponse(status, json)
    }
}

use serde::de::DeserializeOwned;

fn deserialize_from_str<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let data: String = Deserialize::deserialize(deserializer)?;
    serde_json::from_str(&data).map_err(de::Error::custom)
}

#[serde(deny_unknown_fields)]
#[derive(Deserialize, Clone, PartialEq, Debug)]
pub struct StrictGraphQLRequest<S = DefaultScalarValue>
where
    S: ScalarValue,
{
    query: String,
    #[serde(rename = "operationName")]
    operation_name: Option<String>,
    #[serde(bound(deserialize = "InputValue<S>: DeserializeOwned"))]
    #[serde(default = "Option::default")]
    #[serde(deserialize_with = "deserialize_from_str")]
    variables: Option<InputValue<S>>,
}

impl<S> Into<JuniperGraphQLRequest<S>> for StrictGraphQLRequest<S>
where
    S: ScalarValue,
{
    fn into(self) -> JuniperGraphQLRequest<S> {
        JuniperGraphQLRequest::<S>::new(self.query, self.operation_name, self.variables)
    }
}

impl<S> FromRequest for GraphQLRequest<S>
where
    S: ScalarValue + 'static,
{
    type Error = Error;
    type Future = Either<
        LocalBoxFuture<'static, Result<Self, Self::Error>>,
        Ready<Result<Self, Self::Error>>,
    >;
    type Config = ();

    fn from_request(req: &HttpRequest, payload: &mut dev::Payload) -> Self::Future {
        match req.method() {
            &Method::GET => Either::Right(ready(
                web::Query::<StrictGraphQLRequest<S>>::from_query(req.query_string())
                    .map_err(Error::from)
                    .map(|gql_request| {
                        GraphQLRequest(GraphQLBatchRequest::Single(gql_request.into_inner().into()))
                    }),
            )),
            &Method::POST => {
                let content_type_header = req
                    .headers()
                    .get(actix_web::http::header::CONTENT_TYPE)
                    .and_then(|hv| hv.to_str().ok());
                match content_type_header {
                    Some("application/json") =>
                        Either::Left(web::Json::<GraphQLBatchRequest<S>>::from_request(req, payload)
                        .map(|res| res.map_err(Error::from).map(|gql_request| GraphQLRequest(gql_request.into_inner()))).boxed_local()),
                    Some("application/graphql") =>
                        String::from_request(req, payload).map(
                            |res| res.map(
                                |query| GraphQLRequest(GraphQLBatchRequest::Single(JuniperGraphQLRequest::new(query, None, None)))
                            )
                        ).boxed_local().left_future(),
                    _ => Either::Right(err(
                        actix_web::error::ErrorUnsupportedMediaType("GraphQL requests should have content type `application/json` or `application/graphql`")
                    )),
                }
            }
            _ => Either::Right(err(actix_web::error::ErrorMethodNotAllowed(
                "GraphQL requests can only be sent with GET or POST",
            ))),
        }
    }
}

impl Responder for GraphQLResponse {
    type Error = Error;
    type Future = Ready<Result<HttpResponse, Self::Error>>;
    fn respond_to(self, _: &HttpRequest) -> Self::Future {
        let GraphQLResponse(status, body) = self;

        ok(HttpResponse::Ok()
            .status(status)
            .content_type("application/json")
            .body(body))
    }
}

#[cfg(test)]
mod fromrequest_tests;

#[cfg(test)]
mod http_method_tests {
    use super::*;
    use actix_web::{
        http::{Method, StatusCode},
        test::TestRequest,
    };
    use futures::executor::block_on;

    fn check_method(meth: Method, should_succeed: bool) {
        let (req, mut payload) = TestRequest::default().method(meth).to_http_parts();
        let response = block_on(GraphQLRequest::<DefaultScalarValue>::from_request(
            &req,
            &mut payload,
        ));
        match response {
            Err(e) => {
                let status = e.as_response_error().error_response().status();
                if should_succeed {
                    assert_ne!(status, StatusCode::METHOD_NOT_ALLOWED);
                } else {
                    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
                }
            }
            _ => assert!(should_succeed),
        }
    }

    #[test]
    fn test_get() {
        check_method(Method::GET, true);
    }

    #[test]
    fn test_post() {
        check_method(Method::POST, true);
    }

    #[test]
    fn test_put() {
        check_method(Method::PUT, false);
    }

    #[test]
    fn test_patch() {
        check_method(Method::PATCH, false);
    }

    #[test]
    fn test_delete() {
        check_method(Method::DELETE, false);
    }
}

#[cfg(test)]
mod contenttype_tests {
    use super::*;
    use actix_web::{
        http::{header, StatusCode},
        test::TestRequest,
    };

    fn check_content_type(content_type: &str, should_succeed: bool) {
        let (req, mut payload) = TestRequest::post()
            .header(header::CONTENT_TYPE, content_type)
            .to_http_parts();
        let response = futures::executor::block_on(
            GraphQLRequest::<DefaultScalarValue>::from_request(&req, &mut payload),
        );
        match response {
            Err(e) => {
                let status = e.as_response_error().error_response().status();
                if should_succeed {
                    assert_ne!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
                } else {
                    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
                }
            }
            _ => assert!(should_succeed),
        }
    }

    #[test]
    fn check_json() {
        check_content_type("application/json", true);
    }

    #[test]
    fn check_graphql() {
        check_content_type("application/graphql", true);
    }

    #[test]
    fn check_invalid_content_types() {
        check_content_type("text/plain", false);
        check_content_type("multipart/form-data", false);
        check_content_type("foobarbaz", false);
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use actix_web::{
//         guard,
//         test::{self, start, TestServer},
//         web, App,
//     };
//     use futures::executor::block_on;
//     use juniper::{
//         http::tests::{run_http_test_suite, HTTPIntegration, TestResponse},
//         tests::{model::Database, schema::Query},
//         EmptyMutation, RootNode,
//     };
//     use std::sync::Arc;

//     type Schema = RootNode<'static, Query, EmptyMutation<Database>>;

//     struct Data {
//         schema: Schema,
//         context: Database,
//     }

//     struct TestActixWebIntegration {
//         srv: TestServer,
//     }

//     impl TestActixWebIntegration {
//         fn new(srv: TestServer) -> TestActixWebIntegration {
//             TestActixWebIntegration { srv }
//         }

//         fn test_endpoint(&self, endpoint: &str, expected: &str) {
//             println!("URL: {}", endpoint);
//             let request = self.srv.get(endpoint);
//             let body = block_on(
//                 request
//                     .send()
//                     .then(|response| response.expect(&format!("failed GET {}", endpoint)).body())
//                     .map(|body| String::from_utf8(body.unwrap().to_vec()).unwrap()),
//             );

//             assert_eq!(body, expected);
//         }
//     }

//     impl HTTPIntegration for TestActixWebIntegration {
//         fn get(&self, url: &str) -> TestResponse {
//             let request = self.srv.get(url);
//             block_on(request.send().then(make_test_response)).expect(&format!("failed GET {}", url))
//         }

//         fn post(&self, url: &str, body: &str) -> TestResponse {
//             let request = self
//                 .srv
//                 .post(url)
//                 .header(awc::http::header::CONTENT_TYPE, "application/json");
//             block_on(request.send_body(body.to_string()).then(make_test_response))
//                 .expect(&format!("failed POST {}", url))
//         }
//     }

//     type Resp = Result<
//         awc::ClientResponse<
//             actix_http::encoding::Decoder<actix_http::Payload<actix_http::PayloadStream>>,
//         >,
//         awc::error::SendRequestError,
//     >;

//     async fn make_test_response(
//         response: Resp,
//     ) -> Result<TestResponse, awc::error::SendRequestError> {
//         let mut response = response?;
//         let status_code = response.status().as_u16() as i32;
//         let body = response
//             .body()
//             .await
//             .ok()
//             .and_then(|body| String::from_utf8(body.to_vec()).ok());
//         let content_type_header = response
//             .headers()
//             .get(actix_web::http::header::CONTENT_TYPE);
//         let content_type = content_type_header
//             .and_then(|ct| ct.to_str().ok())
//             .unwrap_or_default()
//             .to_string();

//         Ok(TestResponse {
//             status_code,
//             body,
//             content_type,
//         })
//     }

//     #[actix_rt::test]
//     async fn test_actix_web_integration() {
//         let schema = Schema::new(Query, EmptyMutation::<Database>::new());
//         let context = Database::new();
//         let data = Arc::new(Data { schema, context });

//         let srv = start(move || {
//             App::new().data(data.clone()).service(
//                 web::resource("/")
//                     .guard(guard::Any(guard::Get()).or(guard::Post()))
//                     .to(|st: web::Data<Arc<Data>>, data: GraphQLRequest| {
//                         ready(data.execute(&st.schema, &st.context))
//                     }),
//             )
//         });

//         println!("Srv created");

//         let sync_test = TestActixWebIntegration::new(srv);
//         run_http_test_suite(&sync_test);

//         // #[cfg(feature = "async")]
//         // {
//         //     let mut srv = start(|| {
//         //         App::new().service(web::resource("/").to(
//         //             |st: web::Data<Arc<Data>>, data: GraphQLRequest| {
//         //                 data.execute_async(&st.schema, &st.context)
//         //             },
//         //         ))
//         //     });
//         //     let async_test = TestActixWebIntegration::new(srv);
//         //     run_http_test_suite(&async_test);
//         // }

//         // let mut srv = start(move || {
//         //     App::new()
//         //         .service(
//         //             web::resource("/graphiql")
//         //                 .route(web::get().to(move || graphiql_source(protocol_base_url))),
//         //         )
//         //         .service(
//         //             web::resource("/playground")
//         //                 .route(web::get().to(move || playground_source(protocol_base_url))),
//         //         )
//         // });

//         // let tester = TestActixWebIntegration::new(srv);

//         // tester.test_endpoint("graphiql", &graphiql::graphiql_source(protocol_base_url));

//         // tester.test_endpoint(
//         //     "playground",
//         //     &playground::playground_source(protocol_base_url),
//         // );
//     }
// }
