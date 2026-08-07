#[macro_export]
#[cfg(feature = "esp32s3")]
macro_rules! gpio_pins {
    ($per:ident) => {
        (
            Hub75Pins8 {
                red1: $per.GPIO1.degrade(),   //D0
                grn1: $per.GPIO2.degrade(),   //D1
                blu1: $per.GPIO3.degrade(),   //D2
                red2: $per.GPIO4.degrade(),   //D3
                grn2: $per.GPIO5.degrade(),   //D4
                blu2: $per.GPIO6.degrade(),   //D5
                clock: $per.GPIO44.degrade(), //D7
                blank: $per.GPIO8.degrade(),  //D9
                latch: $per.GPIO43.degrade(), //D6
            },
            $per.GPIO7.degrade(), //D8
        )
    };
}

// Alternative pin config for the 3x3 panel
// #[macro_export]
// #[cfg(feature = "esp32s3")]
// macro_rules! gpio_pins {
//     ($per:ident) => {
//         (
//             Hub75Pins8 {
//                 red1: $per.GPIO42.degrade(),
//                 grn1: $per.GPIO41.degrade(),
//                 blu1: $per.GPIO40.degrade(),
//                 red2: $per.GPIO38.degrade(),
//                 grn2: $per.GPIO39.degrade(),
//                 blu2: $per.GPIO12.degrade(),
//                 clock: $per.GPIO2.degrade(),
//                 blank: $per.GPIO14.degrade(),
//                 latch: $per.GPIO47.degrade(),
//             },
//             $per.GPIO45.degrade(),
//         )
//     };
// }

#[macro_export]
#[cfg(feature = "esp32c6")]
macro_rules! gpio_pins {
    ($per:ident) => {
        (
            Hub75Pins8 {
                red1: $per.GPIO0.degrade(),   //D0
                grn1: $per.GPIO1.degrade(),   //D1
                blu1: $per.GPIO2.degrade(),   //D2
                red2: $per.GPIO21.degrade(),  //D3
                grn2: $per.GPIO22.degrade(),  //D4
                blu2: $per.GPIO23.degrade(),  //D5
                clock: $per.GPIO17.degrade(), //D7
                blank: $per.GPIO20.degrade(), //D9
                latch: $per.GPIO16.degrade(), //D6
            },
            $per.GPIO19.degrade(), //D8
        )
    };
}
