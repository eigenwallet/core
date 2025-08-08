struct eventloop {
    struct receiver reciever;

}

void loop(struc eventloop * eventloop) {
    while (true) {
        struct change* change;
        struct network_event *event;

        if (recv(network, event)) {
            // ..
        }
        if (recv(receiver, change)) {
            //
        }
    }
}


struct eigensync {
    struct change *changes;
    struct sender *sender;
};

int main() {
    struct eigensync = create_eigensync();


    eigensync_send_update(&eigensync, &change);
}



struct eigensync create_eigensync() {
    struct receiver* receiver;
    struct sender* sender;

    init_channel(&receiver, &sender);

    tokio::mpsc::channel()

    tokio::spawn(move || async {
        eventloop(receiver).loop().await
    })

    return struct eigensync { .sender=sender };
}

void eigensync_send_update(struct eigensync *eigensync, struct change* change) {
    send(eigensync->sender, change);
}