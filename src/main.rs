//! Compute@Edge A/B Test demo.
use config::{Config, FileFormat};

use fastly::http::{header, Method, StatusCode};
use fastly::{mime, Dictionary, Error, Request, Response};

use fastly::error::BufferKind::{HeaderName, HeaderValue};
use fastly::http::header::{COOKIE, SET_COOKIE};
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand::{thread_rng, Rng};
use std::collections::HashMap;

/// The name of a backend server associated with this service.
///
/// This should be changed to match the name of your own backend. See the the `Hosts` section of
/// the Fastly WASM service UI for more information.
const BACKEND: &str = "cb_google_backend";

const VARIANT_COOKIE_KEY: &str = "ab_variant";
const VARIANT_DICT_NAME: &str = "variants";
const WEIGHTS_DICT_NAME: &str = "weights";

const VARIANTS: [&'static str; 2] = ["A", "B"];

/// The entry point for your application.
///
/// This function is triggered when your service receives a client request. It could be used to
/// route based on the request properties (such as method or path), send the request to a backend,
/// make completely new requests, and/or generate synthetic responses.
///
/// If `main` returns an error, a 500 error response will be delivered to the client.
#[fastly::main]
fn main(mut req: Request) -> Result<Response, Error> {
    // Make sure we are running the version we think we are.
    println!("AB demo version:{}", get_version());

    // Filter request methods...
    match req.get_method() {
        // Allow GET and HEAD requests.
        &Method::GET | &Method::HEAD => (),

        // Accept PURGE requests; it does not matter to which backend they are sent.
        m if m == "PURGE" => return Ok(req.send(BACKEND)?),

        // Deny anything else.
        _ => {
            return Ok(Response::from_status(StatusCode::METHOD_NOT_ALLOWED)
                .with_header(header::ALLOW, "GET, HEAD")
                .with_body_text_plain("This method is not allowed\n"))
        }
    };

    // Pattern match on the path.
    match req.get_path() {
        "/" => {
            let default_header = fastly::http::HeaderValue::from_static("");
            let cookies = req.get_header(COOKIE).unwrap_or(&default_header);
            println!("Cookies: {:?}", cookies);
            let cookie_val = get_cookie_value(&req, VARIANT_COOKIE_KEY);

            // If the cookie is set that is the variant we use, otherwise we randomly select
            // a new variant.
            let variant = match cookie_val {
                Some(variant) => variant,
                None => {
                    // Get the weights out of the edge dictionary and push them into a vector for
                    // the WeightedIndex object.
                    let weights_dict = Dictionary::open(WEIGHTS_DICT_NAME);
                    let mut weights = Vec::new();
                    for variant in &VARIANTS {
                        let weight = weights_dict.get(variant).unwrap().parse::<i32>().unwrap();
                        weights.push(weight);
                    }

                    // Create a distribution based on the weights.
                    let dist = WeightedIndex::new(&weights).unwrap();
                    let mut rng = thread_rng();
                    // Finally assig the randomly selected varient back to the variant variable.
                    VARIANTS[dist.sample(&mut rng)].to_string()
                }
            };

            println!("Variant: {}", variant);
            let variant_dict = Dictionary::open(VARIANT_DICT_NAME);
            // We could do some error handling if the value is not in the dictionary but we will
            // assume the dictionary is properly set up her and panic if it's not.
            let variant_url = variant_dict.get(variant.as_str()).unwrap();
            req.set_url(variant_url);
            let mut resp = req.send(BACKEND)?;
            resp.set_header(SET_COOKIE, format!("{}={}", VARIANT_COOKIE_KEY, variant));
            resp.set_header("Access-Control-Allow-Origin", "*");
            Ok(resp)
        }

        // Let all other requests fall through to the backend.
        _ => {
            let mut resp = req.send(BACKEND)?;
            Ok(resp)
        },
    }
}

// Utility function to get a value given a key
// Returns Some(cookie value) if the cookie exists, otherwise None.
fn get_cookie_value(req: &Request, key: &str) -> Option<String> {
    let cookies = req.get_header_str(COOKIE);
    // If there was not a 'cookies' header the match will just return none.
    match cookies {
        None => None,
        Some(cookies) => {
            let parsed_cookie_val: HashMap<&str, &str> = cookies
                .split("; ")
                .filter_map(|kv| {
                    kv.find("=").map(|index| {
                        let (key, value) = kv.split_at(index);
                        let key = key;
                        let value = &value[1..];
                        (key, value)
                    })
                })
                .collect();

            match parsed_cookie_val.get(key) {
                None => None,
                Some(val) => Some((*val).to_string()),
            }
        }
    }
}


/// This function reads the fastly.toml file and gets the deployed version. This is only run at
/// compile time. Since we bump the version number after building (during the deploy) we return
/// the version incremented by one so the version returned will match the deployed version.
/// NOTE: If the version is incremented by Tango this might be inaccurate.
fn get_version() -> i32 {
    Config::new()
        .merge(config::File::from_str(
            include_str!("../fastly.toml"), // assumes the existence of fastly.toml
            FileFormat::Toml,
        ))
        .unwrap()
        .get_str("version")
        .unwrap()
        .parse::<i32>()
        .unwrap_or(0)
        + 1
}
