use std::future::Future;

use bs_cordl::System::Threading::Tasks::Task_1;
use quest_hook::libil2cpp::Gc;


pub trait Il2CPPFutureAwaitable {
    type Output;

    fn check_task(self: std::pin::Pin<&mut Self>) -> std::task::Poll<Self::Output>;

    fn into_awaitable(self) -> Il2CppFuture<Self>
    where
        Self: Sized,
    {
        Il2CppFuture(self)
    }
}

impl<T> Il2CPPFutureAwaitable for Gc<Task_1<T>>
where
    T: quest_hook::libil2cpp::Type
        + quest_hook::libil2cpp::Argument
        + quest_hook::libil2cpp::Returned,
{
    type Output = quest_hook::libil2cpp::Result<T>;

    fn check_task(mut self: std::pin::Pin<&mut Self>) -> std::task::Poll<Self::Output> {
        if self.get_IsCompleted()? {
            std::task::Poll::Ready(self.get_Result())
        } else {
            std::task::Poll::Pending
        }
    }
}

/// Wrapper type to implement `Future` for Il2CPP Tasks
#[repr(transparent)]
pub struct Il2CppFuture<T: Il2CPPFutureAwaitable>(T);

impl<T: Il2CPPFutureAwaitable> Future for Il2CppFuture<T> {
    type Output = T::Output;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // Safe because we're pinning the field of a pinned struct
        unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.0) }.check_task()
    }
}
