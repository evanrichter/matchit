#![allow(clippy::unknown_clippy_lints)]

use crate::request::FromRequest;
use crate::response::ToReponse;
use futures::future::Future;
use futures::ready;
use hyper::service::Service;
use hyper::{Body, Request, Response};
use std::convert::Infallible;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Async handler factory
pub trait Factory<T, R, O>: Clone + 'static
where
  R: Future<Output = O>,
  O: ToReponse,
{
  fn call(&self, param: T) -> R;
}

impl<F, R, O> Factory<(), R, O> for F
where
  F: Fn() -> R + Clone + 'static,
  R: Future<Output = O>,
  O: ToReponse,
{
  fn call(&self, _: ()) -> R {
    (self)()
  }
}

pub struct Handler<F, T, R, O>
where
  F: Factory<T, R, O>,
  R: Future<Output = O>,
  O: ToReponse,
{
  handler: F,
  _t: PhantomData<(T, R, O)>,
}

impl<F, T, R, O> Handler<F, T, R, O>
where
  F: Factory<T, R, O>,
  R: Future<Output = O>,
  O: ToReponse,
{
  pub fn new(handler: F) -> Self {
    Handler {
      handler,
      _t: PhantomData,
    }
  }
}

impl<F, T, R, O> Clone for Handler<F, T, R, O>
where
  F: Factory<T, R, O>,
  R: Future<Output = O>,
  O: ToReponse,
{
  fn clone(&self) -> Self {
    Handler {
      handler: self.handler.clone(),
      _t: PhantomData,
    }
  }
}

impl<F, T, R, O> Service<(T, crate::Request)> for Handler<F, T, R, O>
where
  F: Factory<T, R, O>,
  R: Future<Output = O>,
  O: ToReponse,
{
  type Response = crate::Response;
  type Error = Infallible;
  type Future = HandlerServiceResponse<R, O>;

  fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  fn call(&mut self, (param, req): (T, crate::Request)) -> Self::Future {
    HandlerServiceResponse {
      fut: self.handler.call(param),
      fut2: None,
      req: Some(req),
    }
  }
}

#[pin_project::pin_project]
pub struct HandlerServiceResponse<T, R>
where
  T: Future<Output = R>,
  R: ToReponse,
{
  #[pin]
  fut: T,
  #[pin]
  fut2: Option<R::Future>,
  req: Option<crate::Request>,
}

impl<T, R> Future for HandlerServiceResponse<T, R>
where
  T: Future<Output = R>,
  R: ToReponse,
{
  type Output = Result<crate::Response, Infallible>;

  fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let this = self.as_mut().project();
    // let (parts, body)  = this.req.unwrap().into_parts();

    // TODO
    if let Some(fut) = this.fut2.as_pin_mut() {
      return match fut.poll(cx) {
        Poll::Ready(Ok(_)) => Poll::Ready(Ok(Response::default())),
        Poll::Pending => Poll::Pending,
        Poll::Ready(Err(_)) => Poll::Ready(Ok(Response::default())),
      };
    }

    match this.fut.poll(cx) {
      Poll::Ready(res) => {
        let fut = res.respond_to(Request::new(Body::default()));
        self.as_mut().project().fut2.set(Some(fut));
        self.poll(cx)
      }
      Poll::Pending => Poll::Pending,
    }
  }
}

/// Extract arguments from handler
pub struct Extract<T: FromRequest, S> {
  service: S,
  _t: PhantomData<T>,
}

impl<T: FromRequest, S> Extract<T, S> {
  pub fn new(service: S) -> Self {
    Extract {
      service,
      _t: PhantomData,
    }
  }
}


impl<T: FromRequest, S> Service<crate::Request> for Extract<T, S>
where
  S: Service<(T, crate::Request), Response = crate::Response, Error = Infallible> + Clone,
{
  type Response = crate::Response;
  type Error = (hyper::Error, crate::Request);
  type Future = ExtractResponse<T, S>;

  fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  fn call(&mut self, req: crate::Request) -> Self::Future {
    let (parts, body)  = req.into_parts();
    let fut = T::from_request(Request::from_parts(parts, body));
    // TODO somehow clone request

    ExtractResponse {
      fut,
      fut_s: None,
      service: self.service.clone(),
    }
  }
}

#[pin_project::pin_project]
pub struct ExtractResponse<T: FromRequest, S: Service<(T, crate::Request)>> {
  service: S,
  #[pin]
  fut: T::Future,
  #[pin]
  fut_s: Option<S::Future>,
}

impl<T: FromRequest, S> Future for ExtractResponse<T, S>
where
  S: Service<(T, crate::Request), Response = crate::Response, Error = Infallible>,
{
  type Output = Result<crate::Response, (hyper::Error, crate::Request)>;

  fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
    let this = self.as_mut().project();

    if let Some(fut) = this.fut_s.as_pin_mut() {
      return fut.poll(cx).map_err(|_| panic!());
    }

    match ready!(this.fut.poll(cx)) {
      Err(e) => {
        // TODO somehow clone request
        let req = Request::new(Body::default());
        Poll::Ready(Err((e.into(), req)))
      }
      Ok(item) => {
        // TODO somehow clone request
        let fut = Some(this.service.call((item, Request::new(Body::default()))));
        self.as_mut().project().fut_s.set(fut);
        self.poll(cx)
      }
    }
  }
}

/// Implement `Factory` for tuples, ie: functions with multiple parameters
macro_rules! factory_tuple ({ $(($n:tt, $T:ident)),+} => {
  impl<Func, $($T,)+ Res, O> Factory<($($T,)+), Res, O> for Func
  where Func: Fn($($T,)+) -> Res + Clone + 'static,
    Res: Future<Output = O>,
    O: ToReponse,
  {
    fn call(&self, param: ($($T,)+)) -> Res {
      (self)($(param.$n,)+)
    }
  }
});

#[rustfmt::skip]
mod m {
  use super::*;

  factory_tuple!((0, A));
  factory_tuple!((0, A), (1, B));
  factory_tuple!((0, A), (1, B), (2, C));
  factory_tuple!((0, A), (1, B), (2, C), (3, D));
  factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E));
  factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F));
  factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G));
  factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H));
  factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I));
  factory_tuple!((0, A), (1, B), (2, C), (3, D), (4, E), (5, F), (6, G), (7, H), (8, I), (9, J));
}

#[cfg(test)]
mod test {
  use crate::handler::Handler;
  use futures::future::ready;
  use hyper::Response;

  #[test]
  fn test() {
    Handler::new(|| ready(Response::default()));
  }
}
