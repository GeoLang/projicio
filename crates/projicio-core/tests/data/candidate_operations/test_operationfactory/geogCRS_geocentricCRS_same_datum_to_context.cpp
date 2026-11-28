TEST(operation, geogCRS_geocentricCRS_same_datum_to_context) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    auto list = CoordinateOperationFactory::create()->createOperations(
        // WGS84 geocentric
        authFactory->createCoordinateReferenceSystem("4978"),
        authFactory->createCoordinateReferenceSystem("4326"), ctxt);
    ASSERT_EQ(list.size(), 1U);
    EXPECT_EQ(list[0]->nameStr(),
              "Conversion from WGS 84 (geocentric) to WGS 84 (geog2D)");
    EXPECT_EQ(list[0]->exportToPROJString(PROJStringFormatter::create().get()),
              "+proj=pipeline +step +inv +proj=cart +ellps=WGS84 +step "
              "+proj=unitconvert +xy_in=rad +xy_out=deg +step +proj=axisswap "
              "+order=2,1");
}

