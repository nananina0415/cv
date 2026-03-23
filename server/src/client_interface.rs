

struct Client {
    // Fields for client interface
}

impl Client {
    fn user_input_stream(&self) -> impl Stream<Item = UserInput> {
        // Implementation for user input stream
    }
    async fn request_http() {}

    fn recieve_udp<T>(data: T) {}
}