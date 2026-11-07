#![feature(impl_trait_in_assoc_type)]

use reqwester::{HasPayload, Payload, Request, requester};
use std::borrow::Cow;
use std::future::{Ready, ready};

#[derive(Debug, Payload)]
#[payload(returns = String)]
pub struct Greet<'a> {
    pub name: &'a str,
    pub suffix: Option<Cow<'static, str>>,
}

#[derive(Debug, Payload)]
#[payload(returns = usize)]
pub struct Count {
    pub items: Vec<u8>,
    pub base: u32,
}

pub struct Fake<P: Payload>(P, P::Output);

impl<P: Payload> HasPayload for Fake<P> {
    type Payload = P;
    fn payload(&self) -> &P { &self.0 }
    fn payload_mut(&mut self) -> &mut P { &mut self.0 }
}

impl<P: Payload> IntoFuture for Fake<P> where P::Output: Clone {
    type Output = Result<P::Output, ()>;
    type IntoFuture = Ready<Self::Output>;
    fn into_future(self) -> Self::IntoFuture { ready(Ok(self.1)) }
}

impl<P: Payload> Request for Fake<P> where P::Output: Clone {
    type Error = ();
    type Send = Ready<Result<P::Output, ()>>;
    type SendRef<'a> = Ready<Result<P::Output, ()>> where Self: 'a;
    fn send(self) -> Self::Send { ready(Ok(self.1)) }
    fn send_ref(&self) -> Self::SendRef<'_> { ready(Ok(self.1.clone())) }
}

#[requester(Send)]
pub trait Api {
    type Error;

    fn greet<'a>(&self, name: &'a str);
    fn count(&self, items: Vec<u8>, base: u32) -> Count;
}

struct Impl;

#[requester]
impl Api for Impl {
    type Error = ();

    // no return type -> `type Greet<'a> = impl Request<..>`
    fn greet<'a>(&self, name: &'a str) {
        Fake(Greet::new(name), format!("hi {name}"))
    }

    // explicit return type -> `type Count = Fake<Count>`
    fn count(&self, items: Vec<u8>, base: u32) -> Fake<Count> {
        let n = items.len() + base as usize;
        Fake(Count::new(items, base), n)
    }
}

fn block<F: Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut f = std::pin::pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => unreachable!(),
    }
}

#[test]
fn works() {
    let owned = String::from("bob");
    let api = Impl;
    assert_eq!(block(api.greet(owned.as_str()).send()), Ok("hi bob".to_string()));
    assert_eq!(block(api.count(vec![1, 2, 3], 4u32).into_future()), Ok(7));
}
